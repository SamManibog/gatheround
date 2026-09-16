use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    sync::mpsc,
};

use axum::http::StatusCode;
use base64::Engine;
use rand::Rng;
use serde::Serialize;
use tokio::{self, sync::oneshot};

use crate::{board::players::PlayerColor, server::PlayerEvent, utils::ecs::DenseComponentManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionKind {
    NonPlayer,
    PlayerController,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionToken([u8; Self::LENGTH]);

impl SessionToken {
    pub const LENGTH: usize = 32;

    pub fn new() -> Self {
        let mut bytes = [0; Self::LENGTH];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// converts into a string that can be sent via a URL
    pub fn to_url_string(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&self.0)
    }

    /// tries to get a token from a string
    pub fn from_str(s: &str) -> Option<Self> {
        let mut bytes = [0; Self::LENGTH];
        match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode_slice(s, &mut bytes) {
            Ok(write_count) => {
                if write_count == Self::LENGTH {
                    Some(Self(bytes))
                } else {
                    None
                }
            }

            Err(_) => None,
        }
    }
}

#[derive(Debug)]
pub(super) enum InvalidSessionError {
    DoesNotExist,
}

impl Display for InvalidSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::DoesNotExist => "Session token is invalid.",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug)]
pub(super) enum NewPlayerError {
    GameFull,
    NewJoinsDisabled,
    PlayerAlreadyUsed,
    IsAlreadyPlayer,
    SessionDoesNotExist,
}

impl NewPlayerError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::GameFull => StatusCode::LOCKED,
            Self::NewJoinsDisabled => StatusCode::LOCKED,
            Self::PlayerAlreadyUsed => StatusCode::CONFLICT,
            Self::IsAlreadyPlayer => StatusCode::PRECONDITION_FAILED,
            Self::SessionDoesNotExist => StatusCode::FORBIDDEN,
        }
    }
}

impl Display for NewPlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::GameFull => "game full",
            Self::NewJoinsDisabled => "joins disabled",
            Self::PlayerAlreadyUsed => "color taken",
            Self::IsAlreadyPlayer => "is already player",
            Self::SessionDoesNotExist => "session does not exist",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug)]
pub(super) enum NewSessionError {
    NewJoinsDisabled,
}

impl NewSessionError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NewJoinsDisabled => StatusCode::LOCKED,
        }
    }
}

impl Display for NewSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NewJoinsDisabled => "joins disabled",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug)]
struct PlayerState {
    color: PlayerColor,
    update_state: SessionUpdateState,
}

impl PlayerState {
    pub fn new(color: PlayerColor) -> Self {
        Self {
            color,
            update_state: SessionUpdateState::new(),
        }
    }
}

#[derive(Debug)]
enum SessionState {
    Player(Box<PlayerState>),
    NonPlayer(SessionUpdateState),
}

impl SessionState {
    pub fn mark_as_needs_update(&mut self) {
        match self {
            Self::Player(p) => p.update_state.needs_update = true,
            Self::NonPlayer(n) => n.needs_update = true,
        }
    }

    pub fn is_player(&self) -> bool {
        match self {
            Self::Player(_) => true,
            Self::NonPlayer(_) => false,
        }
    }
}

#[derive(Debug)]
struct SessionUpdateState {
    update_request_opt: Option<oneshot::Sender<(StatusCode, String)>>,
    needs_update: bool,
}

impl SessionUpdateState {
    pub fn new() -> Self {
        Self {
            update_request_opt: None,
            needs_update: true,
        }
    }

    pub fn subscribe(&mut self) -> oneshot::Receiver<(StatusCode, String)> {
        if let Some(request) = self.update_request_opt.take() {
            let _ = request.send((StatusCode::BAD_REQUEST, format!("request overwritten")));
        }

        let (sender, receiver) = oneshot::channel();
        self.update_request_opt = Some(sender);
        receiver
    }

    /// attempts to send a completed response
    /// returning true on success and false on failure to send
    pub fn try_complete(&mut self, code: StatusCode, response: String) -> bool {
        if let Some(request) = self.update_request_opt.take() {
            self.needs_update = false;
            let _ = request.send((code, response));
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Serialize)]
struct HostStateResponse<'a> {
    players: &'a [PlayerColor],
    host: Option<PlayerColor>,
}

#[derive(Debug, Serialize)]
struct PlayerStateResponse<'a> {
    players: &'a [PlayerColor],
}

#[derive(Debug, Serialize)]
struct NonPlayerStateResponse<'a> {
    players: &'a [PlayerColor],
    joins_allowed: bool,
}

#[derive(Debug)]
pub(super) struct SessionManager {
    host: Option<(SessionToken, SessionUpdateState)>,

    player_event_sender: mpsc::Sender<PlayerEvent>,

    permanent_session_count: usize,
    session_states: HashMap<SessionToken, SessionState>,
    non_player_sessions: VecDeque<SessionToken>,

    players: DenseComponentManager<PlayerColor, SessionToken>,
    joins_blocked: bool,
}

impl SessionManager {
    pub const NON_PLAYER_SESSION_MAX: usize = PlayerColor::COLOR_COUNT * 2;

    pub fn new(player_event_sender: mpsc::Sender<PlayerEvent>) -> Self {
        Self {
            host: None,

            permanent_session_count: 0,
            player_event_sender,
            session_states: HashMap::with_capacity(PlayerColor::COLOR_COUNT),
            non_player_sessions: VecDeque::with_capacity(Self::NON_PLAYER_SESSION_MAX),

            players: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            joins_blocked: true,
        }
    }

    pub fn allow_joins(&mut self) {
        if self.joins_blocked {
            self.joins_blocked = false;

            self.mark_non_players_need_update();
            self.send_all_non_players_state();
        }
    }

    pub fn block_joins(&mut self) {
        if !self.joins_blocked {
            self.joins_blocked = true;

            self.mark_non_players_need_update();
            self.send_all_non_players_state();
        }
    }

    /// builds an update request
    pub fn build_player_state_response(players: &[PlayerColor]) -> String {
        let state = PlayerStateResponse { players };

        serde_json::to_string(&state).unwrap()
    }

    /// builds an update request for non-players
    pub fn build_non_player_state_response(joins_allowed: bool, players: &[PlayerColor]) -> String {
        let state = NonPlayerStateResponse {
            players,
            joins_allowed,
        };

        serde_json::to_string(&state).unwrap()
    }

    /// builds a host update request
    pub fn build_host_state_response(host: Option<PlayerColor>, players: &[PlayerColor]) -> String {
        let state = HostStateResponse { players, host };

        serde_json::to_string(&state).unwrap()
    }

    /// propagate an update that pertains to everyone
    /// DOES NOT mark any player as needing an update
    /// only unmarks need_update IF UPDATE IS SENT
    pub fn send_global_state(&mut self) {
        let players: Vec<PlayerColor> = self.players.entity_iter().collect();

        // propagate player update
        let joins_allowed = self.joins_allowed();
        for session in self.session_states.values_mut() {
            match session {
                SessionState::Player(p) => p.update_state.try_complete(
                    StatusCode::ACCEPTED,
                    Self::build_player_state_response(players.as_slice()),
                ),

                SessionState::NonPlayer(n) => n.try_complete(
                    StatusCode::ACCEPTED,
                    Self::build_non_player_state_response(joins_allowed, players.as_slice()),
                ),
            };
        }

        self.send_host_state();
    }

    pub fn joins_allowed(&self) -> bool {
        !self.joins_blocked && self.permanent_session_count < PlayerColor::COLOR_COUNT
    }

    /// propagate an update that pertains to all non players
    pub fn send_all_non_players_state(&mut self) {
        let players: Vec<PlayerColor> = self.players.entity_iter().collect();

        // propagate player update
        let joins_allowed = self.joins_allowed();
        for session in self.session_states.values_mut() {
            match session {
                SessionState::Player(_) => true,

                SessionState::NonPlayer(n) => n.try_complete(
                    StatusCode::ACCEPTED,
                    Self::build_non_player_state_response(joins_allowed, players.as_slice()),
                ),
            };
        }
    }

    /// propagate an update that pertains to one non-player
    pub fn send_non_player_state(&mut self, token: &SessionToken) {
        let players: Vec<PlayerColor> = self.players.entity_iter().collect();

        // propagate player update
        let joins_allowed = self.joins_allowed();

        if let Some(session) = self.session_states.get_mut(token) {
            match session {
                SessionState::Player(_) => true,

                SessionState::NonPlayer(n) => n.try_complete(
                    StatusCode::ACCEPTED,
                    Self::build_non_player_state_response(joins_allowed, players.as_slice()),
                ),
            };
        }
    }

    /// propagate an update that pertains to players
    /// DOES NOT mark any player as needing an update
    /// only unmarks need_update IF UPDATE IS SENT
    pub fn send_all_players_state(&mut self) {
        let players: Vec<PlayerColor> = self.players.entity_iter().collect();

        // propagate player update
        for session in self.session_states.values_mut() {
            match session {
                SessionState::Player(p) => p.update_state.try_complete(
                    StatusCode::ACCEPTED,
                    Self::build_player_state_response(players.as_slice()),
                ),
                SessionState::NonPlayer(_) => true,
            };
        }
    }

    /// propagate an update to a single player
    /// DOES NOT mark the player as needing an update
    /// only unmarks need_update IF UPDATE IS SENT
    pub fn send_player_state(&mut self, token: &SessionToken) {
        let session = if let Some(session) = self.session_states.get_mut(token) {
            session
        } else {
            return;
        };
        let update_state = match session {
            SessionState::Player(p) => &mut p.update_state,
            SessionState::NonPlayer(_) => return,
        };

        let players: Vec<PlayerColor> = self.players.entity_iter().collect();

        update_state.try_complete(
            StatusCode::ACCEPTED,
            Self::build_player_state_response(players.as_slice()),
        );
    }

    /// propagate an update that pertains only to the host
    /// DOES NOT mark host as needing an update
    /// only unmarks need_update IF UPDATE IS SENT
    pub fn send_host_state(&mut self) {
        let (host_token, session) = if let Some((host_token, session)) = self.host.as_mut() {
            (host_token, session)
        } else {
            return;
        };

        let host_color = match self.session_states.get(host_token) {
            Some(session) => match session {
                SessionState::Player(p) => Some(p.color),
                SessionState::NonPlayer(_) => None,
            },
            None => None,
        };
        let players: Vec<PlayerColor> = self.players.entity_iter().collect();

        session.try_complete(
            StatusCode::ACCEPTED,
            Self::build_host_state_response(host_color, &players),
        );
    }

    // marks all players as needing an update
    fn mark_players_need_update(&mut self) {
        for state in self.session_states.values_mut() {
            match state {
                SessionState::Player(p) => p.update_state.needs_update = true,
                _ => (),
            }
        }
    }

    // marks all non players as needing an update
    fn mark_non_players_need_update(&mut self) {
        for state in self.session_states.values_mut() {
            match state {
                SessionState::NonPlayer(n) => n.needs_update = true,
                _ => (),
            }
        }
    }

    // marks the host as needing an update
    fn mark_host_needs_update(&mut self) {
        if let Some((_, host_state)) = self.host.as_mut() {
            host_state.needs_update = true;
        }
    }

    // marks all sessions as needing an update
    fn mark_all_need_update(&mut self) {
        for state in self.session_states.values_mut() {
            state.mark_as_needs_update();
        }
        self.mark_host_needs_update();
    }

    /// checks if a token controlls a player
    pub fn is_player(&self, token: &SessionToken) -> bool {
        match self.session_states.get(token) {
            Some(session_opt) => session_opt.is_player(),
            None => false,
        }
    }

    /// check if a token corresponds to a session
    pub fn is_session(&self, token: &SessionToken) -> bool {
        self.session_states.get(token).is_some()
    }

    /// checks if a host session exists
    pub fn has_host(&self) -> bool {
        self.host.is_some()
    }

    /// checks if the given token is the host's token
    pub fn is_host(&mut self, token: &SessionToken) -> bool {
        if let Some((host_token, _)) = &self.host {
            host_token == token
        } else {
            false
        }
    }

    /// subscribe to a session dynamically
    pub fn subscribe_to_update(
        &mut self,
        update_kind: SubscriptionKind,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<(StatusCode, String)>> {
        match update_kind {
            SubscriptionKind::NonPlayer => self.subscribe_to_non_player_update(token),
            SubscriptionKind::PlayerController => self.subscribe_to_controller_update(token),
            SubscriptionKind::Host => self.subscribe_to_host_update(token),
        }
    }

    /// send an update dynamically
    pub fn send_state(&mut self, update_kind: SubscriptionKind, token: &SessionToken) {
        match update_kind {
            SubscriptionKind::NonPlayer => self.send_non_player_state(token),
            SubscriptionKind::PlayerController => self.send_player_state(token),
            SubscriptionKind::Host => self.send_host_state(),
        }
    }

    /// subscribe session by token to next nonplayer update
    pub fn subscribe_to_non_player_update(
        &mut self,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<(StatusCode, String)>> {
        let session = if let Some(session) = self.session_states.get_mut(token) {
            session
        } else {
            return None;
        };

        match session {
            SessionState::Player(_) => None,
            SessionState::NonPlayer(n) => Some(n.subscribe()),
        }
    }

    /// subscribe session by token to next controller update
    pub fn subscribe_to_controller_update(
        &mut self,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<(StatusCode, String)>> {
        let session = if let Some(session) = self.session_states.get_mut(token) {
            session
        } else {
            return None;
        };

        match session {
            SessionState::Player(p) => Some(p.update_state.subscribe()),
            SessionState::NonPlayer(_) => None,
        }
    }

    /// subscribe host to next host update
    pub fn subscribe_to_host_update(
        &mut self,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<(StatusCode, String)>> {
        if let Some((host_token, host_state)) = &mut self.host {
            if host_token != token {
                return None;
            }

            Some(host_state.subscribe())
        } else {
            return None;
        }
    }

    /// creates a new session, returning its token on success
    pub fn new_session(&mut self) -> Result<SessionToken, NewSessionError> {
        if !self.joins_allowed() {
            return Err(NewSessionError::NewJoinsDisabled);
        }

        if self.non_player_sessions.len() >= Self::NON_PLAYER_SESSION_MAX {
            if let Some(kicked_token) = self.non_player_sessions.pop_front() {
                let session = self.session_states.get_mut(&kicked_token).unwrap();
                match session {
                    SessionState::NonPlayer(n) => {
                        n.try_complete(
                            StatusCode::REQUEST_TIMEOUT,
                            String::from("color selection took too long"),
                        );
                    }
                    SessionState::Player(_) => unreachable!(),
                }
            }
        }

        let token = SessionToken::new();
        self.session_states.insert(
            token.clone(),
            SessionState::NonPlayer(SessionUpdateState::new()),
        );
        self.non_player_sessions.push_back(token.clone());
        Ok(token)
    }

    /// promotes the given non-player session to a player of the given color
    pub fn promote_to_player(
        &mut self,
        token: &SessionToken,
        color: PlayerColor,
    ) -> Result<(), NewPlayerError> {
        let is_host = self.is_host(token);
        if !is_host {
            if !self.joins_allowed() {
                return Err(NewPlayerError::NewJoinsDisabled);
            }

            if self.permanent_session_count >= PlayerColor::COLOR_COUNT {
                return Err(NewPlayerError::GameFull);
            }

            if self.players.contains(color) {
                return Err(NewPlayerError::PlayerAlreadyUsed);
            }
        }

        let session = match self.session_states.get_mut(token) {
            Some(s) => s,
            None => return Err(NewPlayerError::SessionDoesNotExist),
        };

        if session.is_player() {
            return Err(NewPlayerError::IsAlreadyPlayer);
        }

        self.non_player_sessions.remove(
            self.non_player_sessions
                .iter()
                .position(|t| t == token)
                .unwrap(),
        );

        *session = SessionState::Player(Box::new(PlayerState::new(color)));
        self.players.insert(color, token.clone());

        if !is_host {
            self.permanent_session_count += 1;
        }

        self.player_event_sender
            .send(PlayerEvent::Joined(color))
            .expect("Could not send new player to main thread.");

        if self.is_host(token) {
            self.player_event_sender
                .send(PlayerEvent::SetHost(color))
                .expect("Could not set host event to main thread.");
        }

        self.mark_all_need_update();
        self.send_global_state();

        Ok(())
    }

    /// gets the player color associated with the given session
    pub fn session_player_color(
        &mut self,
        token: &SessionToken,
    ) -> Result<PlayerColor, InvalidSessionError> {
        match self.session_states.get(token) {
            Some(session) => match &session {
                SessionState::Player(player) => Ok(player.color),
                SessionState::NonPlayer(_) => Err(InvalidSessionError::DoesNotExist),
            },
            None => Err(InvalidSessionError::DoesNotExist),
        }
    }

    /// sets the host to the given token
    /// registering the new session if the token is not already in use
    pub fn set_host(&mut self, token: SessionToken) {
        // reserve host's token
        if !self.session_states.contains_key(&token) {
            self.session_states.insert(
                token.clone(),
                SessionState::NonPlayer(SessionUpdateState::new()),
            );
            self.non_player_sessions.push_back(token.clone());
            self.permanent_session_count += 1;
        }

        if let Some((_, update_state)) = self.host.take() {
            if let Some(request) = update_state.update_request_opt {
                request
                    .send((
                        StatusCode::FORBIDDEN,
                        String::from("Host privileges revoked"),
                    ))
                    .expect("Unable to send host change message.");
            }
        }

        self.host = Some((token, SessionUpdateState::new()));
    }

    /// kicks the player of the given color
    pub fn kick_player(&mut self, player: PlayerColor) {
        if let Some(token) = self.players.remove(player)
            && Some(&token) != (&self.host).as_ref().map(|(t, _)| t)
        {
            let session_state = self.session_states.remove(&token).unwrap();
            self.permanent_session_count -= 1;

            match session_state {
                SessionState::Player(p) => {
                    if let Some(request) = p.update_state.update_request_opt {
                        request
                            .send((StatusCode::FORBIDDEN, String::from("You have been kicked")))
                            .expect("Unable to send kick message");
                    }

                    self.mark_all_need_update();
                    self.send_global_state();
                }

                _ => unreachable!(),
            }
        }
    }
}
