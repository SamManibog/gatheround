use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    str::FromStr,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use axum::{
    extract::{
        WebSocketUpgrade,
        ws::{Message, Utf8Bytes, WebSocket},
    },
    http::StatusCode,
    response::Response,
};
use base64::Engine;
use rand::{Rng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use tokio::{self, sync::oneshot};

use crate::{
    board::players::PlayerColor,
    input::{DeserializableInputEvent, InputEvent, PlayerInputEvent},
    utils::ecs::{DenseComponentManager, EntityTrait},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    NonPlayer,
    PlayerController,
    Host,
}

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct HostJoinCode(u32);

impl FromStr for HostJoinCode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();

        let num_result = str::parse::<u32>(trimmed);
        let num = match num_result {
            Ok(n) => n,
            Err(_) => return Err(()),
        };

        if trimmed.len() as u32 != Self::LENGTH {
            return Err(());
        }

        if num >= Self::MAX_JOIN_CODE {
            return Err(());
        }

        Ok(Self(num))
    }
}

impl Display for HostJoinCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let width = Self::LENGTH as usize;
        write!(f, "{:0>width$}", self.0)
    }
}

impl EntityTrait for HostJoinCode {
    fn id(&self) -> u32 {
        self.0
    }

    fn from_id(id: usize) -> Self {
        Self(id as u32)
    }
}

impl HostJoinCode {
    pub const LENGTH: u32 = 4;
    pub const MAX_JOIN_CODE: u32 = 10_u32.pow(Self::LENGTH) - 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionToken([u8; Self::LENGTH]);

impl SessionToken {
    pub const LENGTH: usize = 16;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerEvent {
    EnterLobby,
    EnterGameSelector,

    SetHost(PlayerColor),
    Joined(PlayerColor),
    TryKickPlayer(PlayerColor),
}

#[derive(Debug, Clone, Deserialize)]
struct DeserializableSessionSocketHandshake {
    color: Option<String>,
    token: String,
}

#[derive(Debug)]
struct SessionSocketHandshake {
    color: Option<PlayerColor>,
    token: SessionToken,
}

impl TryFrom<DeserializableSessionSocketHandshake> for SessionSocketHandshake {
    type Error = ();

    fn try_from(value: DeserializableSessionSocketHandshake) -> Result<Self, Self::Error> {
        let color = if let Some(color) = value.color {
            if let Some(player_color) = PlayerColor::from_name(&color) {
                Some(player_color)
            } else {
                return Err(());
            }
        } else {
            None
        };

        let token = if let Some(token) = SessionToken::from_str(&value.token) {
            token
        } else {
            return Err(());
        };

        Ok(Self { color, token })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NewHostCandidateError {
    HostExists,
    HostCodesExhausted,
    SessionDoesNotExist,
}

impl Display for NewHostCandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::HostExists => "host exists",
            Self::HostCodesExhausted => "host codes exhausted",
            Self::SessionDoesNotExist => "session does not exist",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NewPlayerError {
    GameFull,
    NewJoinsDisabled,
    ColorAlreadyUsed,
    SessionDoesNotExist,
}

impl Display for NewPlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::GameFull => "game full",
            Self::NewJoinsDisabled => "joins disabled",
            Self::ColorAlreadyUsed => "color taken",
            Self::SessionDoesNotExist => "session does not exist",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug)]
struct PlayerState {
    color: PlayerColor,
    update_state: SessionUpdateState,
    websocket_handle: oneshot::Sender<()>,
}

impl PlayerState {
    pub fn new(
        color: PlayerColor,
        initial_state: String,
        websocket_handle: oneshot::Sender<()>,
    ) -> Self {
        Self {
            color,
            update_state: SessionUpdateState::new(initial_state),
            websocket_handle,
        }
    }
}

#[derive(Debug)]
struct NonPlayerState {
    update_state: SessionUpdateState,
}

impl NonPlayerState {
    pub fn new(initial_state: String) -> Self {
        Self {
            update_state: SessionUpdateState::new(initial_state),
        }
    }
}

#[derive(Debug)]
enum SessionState {
    Player(Box<PlayerState>),
    NonPlayer(NonPlayerState),
}

impl SessionState {
    pub fn is_player(&self) -> bool {
        match self {
            Self::Player(_) => true,
            Self::NonPlayer(_) => false,
        }
    }

    pub fn color(&self) -> Option<PlayerColor> {
        match self {
            Self::Player(p) => Some(p.color),
            Self::NonPlayer(_) => None,
        }
    }
}

#[derive(Debug)]
struct SessionUpdateState {
    /// the completer for the current subscription, if it exists
    update_completer_opt: Option<oneshot::Sender<(StatusCode, String)>>,

    /// the response to be sent upon the next subscription is registered
    /// This must be none if update_request_opt.is_some(), because the response should be sent as
    /// soon as an update_completer becomes available.
    /// This may be none if update_request_opt.is_none()
    queued_response: Option<(StatusCode, String)>,
}

impl SessionUpdateState {
    pub fn new(value: String) -> Self {
        Self {
            update_completer_opt: None,
            queued_response: Some((StatusCode::ACCEPTED, value)),
        }
    }

    /// returns a oneshot event that resolves to a status code and string output
    pub fn subscribe(&mut self) -> oneshot::Receiver<(StatusCode, String)> {
        if let Some(completer) = self.update_completer_opt.take() {
            let _ = completer.send((StatusCode::BAD_REQUEST, format!("request overwritten")));
        }

        let (sender, receiver) = oneshot::channel();
        if let Some(response) = self.queued_response.take() {
            // complete subscription immediately if possible
            let _ = sender.send(response);
        } else {
            // create new subscription
            self.update_completer_opt = Some(sender);
        }
        receiver
    }

    /// attempts to send a completed response
    /// returning true on success and false on failure to send
    pub fn try_complete(&mut self, code: StatusCode, response: String) -> bool {
        if let Some(request) = self.update_completer_opt.take() {
            self.queued_response = None;
            let _ = request.send((code, response));
            true
        } else {
            self.queued_response = Some((code, response));
            false
        }
    }
}

#[derive(Debug)]
struct HostCandidateState {
    code: HostJoinCode,
    acceptance_sender: Option<oneshot::Sender<HostCandidateResponse>>,
}

impl HostCandidateState {
    pub fn new(code: HostJoinCode) -> Self {
        Self {
            code,
            acceptance_sender: None,
        }
    }

    pub fn subscribe(&mut self) -> oneshot::Receiver<HostCandidateResponse> {
        if let Some(s) = self.acceptance_sender.take() {
            let _ = s.send(HostCandidateResponse::Overwritten);
        }
        let (sender, receiver) = oneshot::channel();
        self.acceptance_sender = Some(sender);
        receiver
    }

    pub fn accept(mut self) {
        if let Some(sender) = self.acceptance_sender.take() {
            let _ = sender.send(HostCandidateResponse::Accepted);
        }
    }

    pub fn reject(mut self) {
        if let Some(sender) = self.acceptance_sender.take() {
            let _ = sender.send(HostCandidateResponse::Rejected);
        }
    }
}

impl Drop for HostCandidateState {
    fn drop(&mut self) {
        if let Some(s) = self.acceptance_sender.take() {
            let _ = s.send(HostCandidateResponse::Rejected);
        }
    }
}

#[derive(Debug, Serialize)]
struct HostStateResponse<'a> {
    players: &'a [PlayerColor],
    host: Option<PlayerColor>,
    kicks_allowed: bool,
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
struct HostClaimState {
    code_queue: Vec<HostJoinCode>,
    session_candidates: HashMap<SessionToken, HostCandidateState>,
    join_codes: DenseComponentManager<HostJoinCode, SessionToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HostCandidateResponse {
    Accepted,
    Rejected,
    Overwritten,
}

impl HostClaimState {
    pub fn new() -> Self {
        let mut code_queue = Vec::from_iter(
            (0..=HostJoinCode::MAX_JOIN_CODE)
                .into_iter()
                .map(|i| HostJoinCode(i)),
        );
        code_queue.shuffle(&mut rand::rng());

        Self {
            code_queue,
            session_candidates: HashMap::new(),
            join_codes: DenseComponentManager::with_capacity(HostJoinCode::MAX_JOIN_CODE as usize),
        }
    }

    /// kicks the given candidate
    pub fn kick_candidate(&mut self, token: &SessionToken) {
        if let Some(candidate) = self.session_candidates.remove(token) {
            self.join_codes.remove(candidate.code);
        }
    }

    /// checks if the token belongs to a host candidate
    pub fn is_host_candidate(&self, token: &SessionToken) -> bool {
        self.session_candidates.contains_key(token)
    }

    /// attempts to create a new or get an existing host candidate, fails if candidate count has been exhausted
    /// on success returns the join code
    ///
    /// fails if host codes have been exhausted
    pub fn make_host_candidate(
        &mut self,
        token: SessionToken,
    ) -> Result<HostJoinCode, NewHostCandidateError> {
        if let Some(candidate) = self.session_candidates.get(&token) {
            return Ok(candidate.code.clone());
        }

        let code = match self.code_queue.pop() {
            Some(c) => c,
            None => return Err(NewHostCandidateError::HostCodesExhausted),
        };

        self.join_codes.insert(code.clone(), token.clone());
        self.session_candidates
            .insert(token, HostCandidateState::new(code));

        Ok(code)
    }

    /// subscribes the session token to its host candidate response
    pub fn subscribe_to_candidate_response(
        &mut self,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<HostCandidateResponse>> {
        if let Some(candidate) = self.session_candidates.get_mut(token) {
            Some(candidate.subscribe())
        } else {
            None
        }
    }

    /// attempts to mark the host as accepted,
    /// returns the host's session token on success
    ///
    /// after successful acceptance, you may drop this struct
    pub fn try_accept_host(&mut self, code: HostJoinCode) -> Option<SessionToken> {
        if let Some(token) = self.join_codes.remove(code) {
            // send accepted status
            let host_session = self.session_candidates.remove(&token).unwrap();
            host_session.accept();

            // send rejected status to other hosts
            for token in self.join_codes.component_iter() {
                let state = self.session_candidates.remove(token).unwrap();
                state.reject();
            }
            self.join_codes.clear();

            Some(token)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub(super) struct ServerState {
    host: Option<(SessionToken, SessionUpdateState)>,

    input_sender: mpsc::Sender<PlayerInputEvent>,
    event_sender: mpsc::Sender<ServerEvent>,

    permanent_session_count: usize,
    session_states: HashMap<SessionToken, SessionState>,
    non_player_sessions: VecDeque<SessionToken>,

    host_claim_state: Option<HostClaimState>,

    players: DenseComponentManager<PlayerColor, SessionToken>,
    joins_blocked: bool,
    kicks_allowed: bool,
}

impl ServerState {
    pub const PLAYER_CONNECTION_TIMEOUT: Duration = Duration::from_millis(3000);

    pub const NON_PLAYER_SESSION_MAX: usize = PlayerColor::COLOR_COUNT * 2;

    pub fn new(
        input_sender: mpsc::Sender<PlayerInputEvent>,
        event_sender: mpsc::Sender<ServerEvent>,
    ) -> Self {
        Self {
            host: None,

            input_sender,
            event_sender,

            permanent_session_count: 0,
            session_states: HashMap::with_capacity(PlayerColor::COLOR_COUNT),
            non_player_sessions: VecDeque::with_capacity(Self::NON_PLAYER_SESSION_MAX),

            host_claim_state: Some(HostClaimState::new()),

            players: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            joins_blocked: true,
            kicks_allowed: true,
        }
    }

    /// removes the given session, fails silently only if that session is the host or does not exist
    fn remove_session(&mut self, token: &SessionToken) {
        // dont remove if host
        if Some(token) == self.host.as_ref().map(|(t, _)| t) {
            return;
        }

        // remove session state
        let session_state = match self.session_states.remove(&token) {
            Some(s) => s,
            None => return,
        };

        // cleanup session
        match session_state {
            SessionState::Player(mut p) => {
                self.players.remove(p.color);
                self.permanent_session_count -= 1;

                p.update_state
                    .try_complete(StatusCode::FORBIDDEN, String::from("kicked"));

                self.send_global_state();
            }
            SessionState::NonPlayer(mut n) => {
                n.update_state.try_complete(
                    StatusCode::REQUEST_TIMEOUT,
                    String::from("color selection took too long"),
                );
            }
        }

        // remove host join code
        if let Some(claims) = &mut self.host_claim_state {
            claims.kick_candidate(token);
        }
    }

    pub fn allow_joins(&mut self) {
        if self.joins_blocked {
            self.joins_blocked = false;

            self.send_all_non_players_state();
        }
    }

    pub fn block_joins(&mut self) {
        if !self.joins_blocked {
            self.joins_blocked = true;

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
    pub fn build_host_state_response(
        host: Option<PlayerColor>,
        players: &[PlayerColor],
        kicks_allowed: bool,
    ) -> String {
        let state = HostStateResponse {
            players,
            host,
            kicks_allowed,
        };

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

                SessionState::NonPlayer(n) => n.update_state.try_complete(
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
                SessionState::Player(_) => (),

                SessionState::NonPlayer(n) => {
                    n.update_state.try_complete(
                        StatusCode::ACCEPTED,
                        Self::build_non_player_state_response(joins_allowed, players.as_slice()),
                    );
                }
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

                SessionState::NonPlayer(n) => n.update_state.try_complete(
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
                SessionState::Player(p) => {
                    p.update_state.try_complete(
                        StatusCode::ACCEPTED,
                        Self::build_player_state_response(players.as_slice()),
                    );
                }
                SessionState::NonPlayer(_) => (),
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
            Self::build_host_state_response(host_color, &players, self.kicks_allowed),
        );
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
        session_kind: SessionKind,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<(StatusCode, String)>> {
        match session_kind {
            SessionKind::NonPlayer => self.subscribe_to_non_player_update(token),
            SessionKind::PlayerController => self.subscribe_to_controller_update(token),
            SessionKind::Host => self.subscribe_to_host_update(token),
        }
    }

    /// send an update dynamically
    pub fn send_state(&mut self, update_kind: SessionKind, token: &SessionToken) {
        match update_kind {
            SessionKind::NonPlayer => self.send_non_player_state(token),
            SessionKind::PlayerController => self.send_player_state(token),
            SessionKind::Host => self.send_host_state(),
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
            SessionState::NonPlayer(n) => Some(n.update_state.subscribe()),
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

    /// creats a new session with the given token
    /// fails if new joins are disabled, unless no host exists
    pub fn new_session(&mut self) -> Option<SessionToken> {
        if !self.joins_allowed() && self.has_host() {
            return None;
        }

        // kill old session if necessary
        if self.non_player_sessions.len() >= Self::NON_PLAYER_SESSION_MAX {
            if let Some(kicked_token) = self.non_player_sessions.pop_front() {
                let session = self.session_states.get_mut(&kicked_token).unwrap();
                match session {
                    SessionState::NonPlayer(n) => {
                        n.update_state.try_complete(
                            StatusCode::REQUEST_TIMEOUT,
                            String::from("selection took too long"),
                        );
                    }
                    SessionState::Player(_) => unreachable!(),
                }
                self.remove_session(&kicked_token);
            }
        }

        let token = SessionToken::new();
        self.session_states.insert(
            token.clone(),
            SessionState::NonPlayer(NonPlayerState::new(Self::build_non_player_state_response(
                self.joins_allowed(),
                &self.players.entity_iter().collect::<Vec<PlayerColor>>(),
            ))),
        );
        self.non_player_sessions.push_back(token.clone());

        Some(token)
    }

    /// confirms a player websocket
    /// the player's color on confirmation
    async fn confirm_player_websocket(
        self_arc: Arc<Mutex<Self>>,
        socket: &mut WebSocket,
        cancel_sender: oneshot::Sender<()>,
    ) -> Option<PlayerColor> {
        let handshake_msg = tokio::select! {
            msg = socket.recv() => msg,
            _ = tokio::time::sleep(Self::PLAYER_CONNECTION_TIMEOUT) => {
                return None;
            }
        };

        let handshake = match handshake_msg {
            Some(Ok(Message::Text(message))) => {
                match serde_json::from_str::<DeserializableSessionSocketHandshake>(&message) {
                    Ok(handshake) => match SessionSocketHandshake::try_from(handshake) {
                        Ok(h) => h,
                        Err(_) => return None,
                    },
                    Err(_) => return None,
                }
            }
            _ => return None,
        };

        let mut self_lock = self_arc.lock().expect("unable too lock");
        let session = if let Some(session) = self_lock.session_states.get_mut(&handshake.token) {
            session
        } else {
            return None;
        };
        match session {
            SessionState::Player(p) => {
                p.websocket_handle = cancel_sender;
                Some(p.color)
            }
            SessionState::NonPlayer(_) => {
                let color = if let Some(color) = handshake.color {
                    color
                } else {
                    return None;
                };
                if self_lock
                    .promote_to_player(&handshake.token, color, cancel_sender)
                    .is_err()
                {
                    return None;
                }
                Some(color)
            }
        }
    }

    /// attempts to establish a websocket for the given session
    /// returns none if the token is not a valid
    pub fn establish_player_websocket(
        &mut self,
        ws: WebSocketUpgrade,
        self_arc: Arc<Mutex<Self>>,
    ) -> Response {
        let input_sender = self.input_sender.clone();

        ws.on_upgrade(async move |mut socket| {
            // verify socket's initial response
            let (cancel_sender, mut cancel_receiver) = oneshot::channel::<()>();
            let color = if let Some(color) =
                Self::confirm_player_websocket(self_arc, &mut socket, cancel_sender).await
            {
                let _ = socket
                    .send(Message::Text(Utf8Bytes::from_static("accepted")))
                    .await;
                color
            } else {
                let _ = socket
                    .send(Message::Text(Utf8Bytes::from_static("rejected")))
                    .await;
                let _ = socket.send(Message::Close(None)).await;
                return;
            };

            // main body
            let should_continue = |socket_response| {
                let msg_result = if let Some(msg_result) = socket_response {
                    msg_result
                } else {
                    return false;
                };
                let msg = if let Ok(msg) = msg_result {
                    msg
                } else {
                    return false;
                };

                match msg {
                    Message::Text(t) => {
                        let input_result =
                            serde_json::from_str::<DeserializableInputEvent>(t.as_str());
                        let de_input = match input_result {
                            Ok(input) => input,
                            Err(_) => return true,
                        };
                        let input = match InputEvent::try_from(&de_input) {
                            Ok(input) => input,
                            Err(_) => return true,
                        };
                        let _ = input_sender.send(PlayerInputEvent {
                            player: color,
                            event: input,
                        });
                        true
                    }
                    Message::Close(_) => false,
                    _ => true,
                }
            };

            loop {
                tokio::select! {
                    _ = (&mut cancel_receiver).into_future() => {
                        break;
                    }
                    socket_response = socket.recv() => {
                        if !should_continue(socket_response) {
                            break;
                        }
                    }
                }
            }

            let _ = socket.send(Message::Close(None)).await;
        })
    }

    /// promotes the given non-player session to a player of the given color
    fn promote_to_player(
        &mut self,
        token: &SessionToken,
        color: PlayerColor,
        cancel_sender: oneshot::Sender<()>,
    ) -> Result<(), NewPlayerError> {
        let is_host = self.is_host(token);
        if !is_host {
            if !self.joins_allowed() {
                return Err(NewPlayerError::NewJoinsDisabled);
            }

            if self.permanent_session_count >= PlayerColor::COLOR_COUNT {
                return Err(NewPlayerError::GameFull);
            }
        }

        if self.players.contains(color) {
            return Err(NewPlayerError::ColorAlreadyUsed);
        }

        let session = match self.session_states.get_mut(token) {
            Some(s) => s,
            None => return Err(NewPlayerError::SessionDoesNotExist),
        };
        match session {
            SessionState::Player(_) => (),
            SessionState::NonPlayer(n) => {
                n.update_state
                    .try_complete(StatusCode::GONE, format!("upgraded to player"));
            }
        };

        self.non_player_sessions.remove(
            self.non_player_sessions
                .iter()
                .position(|t| t == token)
                .unwrap(),
        );

        // create new player
        *session = SessionState::Player(Box::new(PlayerState::new(
            color,
            Self::build_player_state_response(
                &self.players.entity_iter().collect::<Vec<PlayerColor>>(),
            ),
            cancel_sender,
        )));
        self.players.insert(color, token.clone());

        if !is_host {
            self.permanent_session_count += 1;
        }

        self.event_sender
            .send(ServerEvent::Joined(color).into())
            .expect("Could not send new player to main thread.");

        if self.is_host(token) {
            self.event_sender
                .send(ServerEvent::SetHost(color).into())
                .expect("Could not set host event to main thread.");
        }

        self.send_global_state();

        Ok(())
    }

    /// subscribes to a host candidate response
    pub fn subscribe_to_host_candidate_response(
        &mut self,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<HostCandidateResponse>> {
        if let Some(claims) = self.host_claim_state.as_mut() {
            claims.subscribe_to_candidate_response(token)
        } else {
            None
        }
    }

    /// checks if the token belongs to a host candidate
    pub fn is_host_candidate(&self, token: &SessionToken) -> bool {
        if let Some(claims) = &self.host_claim_state {
            claims.is_host_candidate(token)
        } else {
            false
        }
    }

    /// creates a new host candidate session, returning a future that resolves to a
    /// HostCandidateResponse on success
    pub fn make_host_candidate(
        &mut self,
        token: SessionToken,
    ) -> Result<HostJoinCode, NewHostCandidateError> {
        if !self.is_session(&token) {
            return Err(NewHostCandidateError::SessionDoesNotExist);
        }

        if let Some(claims) = self.host_claim_state.as_mut() {
            claims.make_host_candidate(token)
        } else {
            Err(NewHostCandidateError::HostExists)
        }
    }

    /// claims the host for the based on a host join code
    /// fails if the host is already set
    /// fails if the host join code is invalid
    pub fn claim_host(&mut self, code: HostJoinCode) -> Result<(), String> {
        if self.has_host() {
            return Err("host already set".to_string());
        }

        let claims = self.host_claim_state.as_mut().unwrap();
        if let Some(token) = claims.try_accept_host(code) {
            self.host = Some((
                token.clone(),
                SessionUpdateState::new(Self::build_host_state_response(
                    None,
                    &Vec::new(),
                    self.kicks_allowed,
                )),
            ));

            // reserve host's token
            self.session_states.insert(
                token.clone(),
                SessionState::NonPlayer(NonPlayerState::new(
                    Self::build_non_player_state_response(
                        self.joins_allowed(),
                        &self.players.entity_iter().collect::<Vec<PlayerColor>>(),
                    ),
                )),
            );
            self.non_player_sessions.push_back(token.clone());
            self.permanent_session_count += 1;

            // clear claims
            self.host_claim_state = None;

            Ok(())
        } else {
            Err("join code invalid".to_string())
        }
    }

    /// sets the host to the given token
    /// fails if the token does not correspond to a session
    pub fn set_host(&mut self, token: SessionToken) -> Result<(), ()> {
        if let Some(session) = self.session_states.get(&token) {
            let _ = match session {
                SessionState::Player(p) => {
                    self.event_sender.send(ServerEvent::SetHost(p.color).into())
                }
                _ => Ok(()),
            };
        } else {
            return Err(());
        }

        if let Some((_, mut update_state)) = self.host.take() {
            update_state.try_complete(StatusCode::FORBIDDEN, String::from("kicked"));
        }
        self.host = Some((
            token.clone(),
            SessionUpdateState::new(Self::build_host_state_response(
                self.session_states
                    .get(&token)
                    .iter()
                    .flat_map(|s| s.color())
                    .next(),
                &self.players.entity_iter().collect::<Vec<PlayerColor>>(),
                self.kicks_allowed,
            )),
        ));

        Ok(())
    }

    /// kicks the player of the given color
    pub fn kick_player(&mut self, player: PlayerColor) {
        let token = self.players.get(player).unwrap().clone();
        self.remove_session(&token);
    }

    /// handles a host command for the given token
    /// returns true on success and false on failure
    /// fails only if the token is not the host
    pub fn handle_host_commands(
        &mut self,
        token: &SessionToken,
        commands: Vec<ServerEvent>,
    ) -> bool {
        if !self.is_host(token) {
            return false;
        }

        for cmd in commands {
            let _ = self.event_sender.send(cmd);
        }

        true
    }

    /// allows kicks
    pub fn set_kicks_allowed_hint(&mut self, allowed: bool) {
        self.kicks_allowed = allowed;

        self.send_host_state();
    }
}
