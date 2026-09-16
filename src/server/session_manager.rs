use std::{collections::HashMap, fmt::Display, sync::mpsc};

use axum::http::StatusCode;
use base64::Engine;
use rand::Rng;
use serde::Serialize;
use tokio::{self, sync::oneshot};

use crate::{board::players::PlayerColor, server::PlayerEvent, utils::ecs::DenseComponentManager};

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
pub(super) enum NewSessionError {
    GameFull,
    NewJoinsBlocked,
    PlayerAlreadyUsed,
    SessionAlreadyEstablished,
}

impl Display for NewSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::GameFull => "game full",
            Self::NewJoinsBlocked => "joins blocked",
            Self::PlayerAlreadyUsed => "color taken",
            Self::SessionAlreadyEstablished => "session exists",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug)]
struct SessionState {
    color: Option<PlayerColor>,
    update_state: SessionUpdateState,
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
            request
                .send((StatusCode::BAD_REQUEST, format!("request overwritten")))
                .expect("Unable to send request overwritten update response.");
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
            request
                .send((code, response))
                .expect("Unable to send update response.");
            true
        } else {
            false
        }
    }
}

impl SessionState {
    pub fn new(color: Option<PlayerColor>) -> Self {
        Self {
            color,
            update_state: SessionUpdateState::new(),
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

#[derive(Debug)]
pub(super) struct SessionManager {
    host: Option<(SessionToken, SessionUpdateState)>,

    player_event_sender: mpsc::Sender<PlayerEvent>,

    session_count: usize,
    session_states: HashMap<SessionToken, SessionState>,

    used_players: DenseComponentManager<PlayerColor, SessionToken>,
    allow_joins: bool,
}

impl SessionManager {
    pub fn new(player_event_sender: mpsc::Sender<PlayerEvent>) -> Self {
        Self {
            host: None,

            session_count: 0,
            player_event_sender,
            session_states: HashMap::with_capacity(PlayerColor::COLOR_COUNT),
            used_players: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            allow_joins: false,
        }
    }

    pub fn allow_joins(&mut self) {
        if !self.allow_joins {
            self.mark_all_need_update();
            self.allow_joins = true;
        }
    }

    pub fn block_joins(&mut self) {
        if self.allow_joins {
            self.allow_joins = false;
            self.mark_all_need_update();
        }
    }

    /// builds an update request
    pub fn build_player_state_response(players: &[PlayerColor]) -> String {
        let state = PlayerStateResponse { players };

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
        let players: Vec<PlayerColor> = self.used_players.entity_iter().collect();

        // propagate player update
        for session in self.session_states.values_mut() {
            session.update_state.try_complete(
                StatusCode::ACCEPTED,
                Self::build_player_state_response(players.as_slice()),
            );
        }

        self.send_host_state();
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
        let players: Vec<PlayerColor> = self.used_players.entity_iter().collect();

        let update_state = &mut session.update_state;
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

        let players: Vec<PlayerColor> = self.used_players.entity_iter().collect();

        session.try_complete(
            StatusCode::ACCEPTED,
            Self::build_host_state_response(
                self.session_states.get(host_token).unwrap().color,
                &players,
            ),
        );
    }

    // marks all sessions as needing an update
    fn mark_players_need_update(&mut self) {
        for state in self.session_states.values_mut() {
            state.update_state.needs_update = true;
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
        self.mark_players_need_update();
        self.mark_host_needs_update();
    }

    pub fn has_host(&self) -> bool {
        self.host.is_some()
    }

    pub fn is_host(&mut self, token: &SessionToken) -> bool {
        if let Some((host_token, _)) = &self.host {
            host_token == token
        } else {
            false
        }
    }

    /// subscribe to next controller update
    pub fn subscribe_to_controller_update(
        &mut self,
        token: &SessionToken,
    ) -> Option<oneshot::Receiver<(StatusCode, String)>> {
        let session = if let Some(session) = self.session_states.get_mut(token) {
            session
        } else {
            return None;
        };

        Some(session.update_state.subscribe())
    }

    /// subscribe to next host update
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

    /// assigns the host a player
    /// panics if there currently is no host
    /// check if a token is host before calling this method to avoid panic
    pub fn new_host_player(&mut self, color: PlayerColor) -> Result<(), NewSessionError> {
        assert!(self.has_host(), "No host, cannot assign host player.");

        if self.session_count >= PlayerColor::COLOR_COUNT {
            return Err(NewSessionError::GameFull);
        }

        if self.used_players.contains(color) {
            return Err(NewSessionError::PlayerAlreadyUsed);
        }

        let host_token = &self.host.as_ref().unwrap().0;
        self.session_states.get_mut(&host_token).unwrap().color = Some(color);
        self.player_event_sender
            .send(PlayerEvent::Joined(color))
            .expect("Could not send new player to main thread.");
        self.player_event_sender
            .send(PlayerEvent::SetHost(color))
            .expect("Could not send new player to main thread.");

        self.mark_all_need_update();
        self.send_global_state();

        Ok(())
    }

    /// creates a new player
    pub fn new_player(&mut self, color: PlayerColor) -> Result<SessionToken, NewSessionError> {
        if !self.allow_joins {
            return Err(NewSessionError::NewJoinsBlocked);
        }

        if self.session_count >= PlayerColor::COLOR_COUNT {
            return Err(NewSessionError::GameFull);
        }

        if self.used_players.contains(color) {
            return Err(NewSessionError::PlayerAlreadyUsed);
        }

        let token = {
            let mut token = SessionToken::new();
            while self.session_states.contains_key(&token) {
                token = SessionToken::new();
            }
            token
        };

        self.session_states
            .insert(token.clone(), SessionState::new(Some(color)));
        self.used_players.insert(color, token.clone());
        self.player_event_sender
            .send(PlayerEvent::Joined(color))
            .expect("Could not send new player to main thread.");

        self.mark_all_need_update();
        self.send_global_state();

        Ok(token.clone())
    }

    /// gets the player color associated with the given session
    pub fn session_player_color(
        &mut self,
        token: &SessionToken,
    ) -> Result<PlayerColor, InvalidSessionError> {
        match self.session_states.get(token) {
            Some(session_opt) => match &session_opt.color {
                Some(color) => Ok(*color),
                None => Err(InvalidSessionError::DoesNotExist),
            },
            None => Err(InvalidSessionError::DoesNotExist),
        }
    }

    /// checks if a token controlls a player
    pub fn is_player(&mut self, token: &SessionToken) -> bool {
        match self.session_states.get(token) {
            Some(session_opt) => session_opt.color.is_some(),
            None => false,
        }
    }

    pub fn set_host(&mut self, token: SessionToken) {
        // reserve host's player
        if !self.session_states.contains_key(&token) {
            self.session_states
                .insert(token.clone(), SessionState::new(None));
            self.session_count += 1;
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

    pub fn kick_player(&mut self, player: PlayerColor) {
        if let Some(token) = self.used_players.remove(player)
            && Some(&token) != (&self.host).as_ref().map(|(t, _)| t)
        {
            let session_state = self.session_states.remove(&token).unwrap();
            self.session_count -= 1;

            if let Some(request) = session_state.update_state.update_request_opt {
                request
                    .send((StatusCode::FORBIDDEN, String::from("You have been kicked")))
                    .expect("Unable to send kick message");
            }

            self.mark_all_need_update();
            self.send_global_state();
        }
    }
}
