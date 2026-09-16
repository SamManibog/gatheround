use std::{
    collections::HashMap,
    fmt::Display,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use base64::Engine;
use rand::Rng;
use serde::{Deserialize, Serialize};
use time::Duration;
use tokio::{self, sync::oneshot, task::JoinHandle};
use tower_http::services::ServeDir;

use crate::{
    ecs::DenseComponentManager,
    input::{DeserializableInputEvent, InputEvent, PlayerInputEvent},
    resources::PlayerColor,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionToken([u8; Self::LENGTH]);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateHostCommand {
    HostConnected,
    PublicCommand(HostCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCommand {
    EnterGameSelectionMode,
    EnterLobbyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerEvent {
    Joined(PlayerColor),
    SetHost(PlayerColor),
}

#[derive(Debug)]
enum InvalidSessionError {
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
enum NewSessionError {
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
struct SessionManager {
    host: Option<(SessionToken, SessionUpdateState)>,

    player_event_sender: Sender<PlayerEvent>,

    session_count: usize,
    session_states: HashMap<SessionToken, SessionState>,

    used_players: DenseComponentManager<PlayerColor, SessionToken>,
    allow_joins: bool,
}

impl SessionManager {
    pub fn new(player_event_sender: Sender<PlayerEvent>) -> Self {
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
        let state = PlayerStateResponse {
            players,
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

#[derive(Debug)]
pub struct ServerState {
    input_sender: Sender<PlayerInputEvent>,
    host_command_sender: Sender<PrivateHostCommand>,
    sessions: Arc<Mutex<SessionManager>>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeserializablePlayerEntry {
    /// the player's name
    pub color: String,
}

#[derive(Debug)]
pub struct GameServer {
    input_receiver: Receiver<PlayerInputEvent>,
    player_event_receiver: Receiver<PlayerEvent>,
    host_command_receiver: Receiver<PrivateHostCommand>,

    sessions: Arc<Mutex<SessionManager>>,

    service_thread: JoinHandle<Result<(), tokio::io::Error>>,
    ip_addr: String,
}

impl GameServer {
    pub const HOST_PATH: &str = "/games/host";
    pub const IS_HOST_API_PATH: &str = "/games/host/is-host";
    pub const HOST_ASSIGNMENT_PATH: &str = "/games/host/claim";
    pub const HOST_EVENT_STREAM_PATH: &str = "/games/events";
    pub const HOST_STATE_PATH: &str = "/games/host/state";
    pub const HOST_LOBBY_PATH: &str = "/games/host/start-lobby";
    pub const HOST_GAME_SELECTION_PATH: &str = "/games/host/select-game";

    pub const CONTROLLER_PATH: &str = "/games";
    pub const IS_PLAYER_API_PATH: &str = "/games/is-player";
    pub const PLAYER_ASSIGNMENT_API_PATH: &str = "/games/get-session";
    pub const CONTROLLER_EVENT_STREAM_PATH: &str = "/games/events";
    pub const CONTROLLER_STATE_PATH: &str = "/games/state";
    pub const INPUT_API_PATH: &str = "/games/input";

    pub const STATIC_CONTROLLER_FILES_PATH: &str = "./static/controller";
    pub const STATIC_HOST_FILES_PATH: &str = "./static/host";

    pub const SESSION_COOKIE_NAME: &str = "session_id";

    pub const COOKIE_LIFETIME: Duration = Duration::days(7);

    fn parse_session_cookie(jar: &CookieJar) -> Result<SessionToken, (StatusCode, String)> {
        let token_string = match jar.get(Self::SESSION_COOKIE_NAME) {
            Some(cookie) => cookie,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    format!("Missing {}", Self::SESSION_COOKIE_NAME),
                ));
            }
        }
        .value_trimmed();
        match SessionToken::from_str(token_string) {
            Some(token) => Ok(token),
            None => Err((
                StatusCode::UNAUTHORIZED,
                format!("Invalid {}", Self::SESSION_COOKIE_NAME),
            )),
        }
    }

    async fn controller_event_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let subscription = {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };

            match sessions_lock.subscribe_to_controller_update(&token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("Not a player")),
            }
        };

        match subscription.await {
            Ok(v) => v,
            Err(msg) => panic!("{msg}"),
        }
    }

    async fn controller_state_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let subscription = {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };

            match sessions_lock.subscribe_to_controller_update(&token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("Not a player")),
            }
        };

        {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };
            sessions_lock.send_player_state(&token);
        }

        match subscription.await {
            Ok(v) => v,
            Err(msg) => panic!("{msg}"),
        }
    }

    async fn host_event_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let subscription = {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };

            match sessions_lock.subscribe_to_host_update(&token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("Note the host")),
            }
        };

        match subscription.await {
            Ok(v) => v,
            Err(msg) => panic!("{msg}"),
        }
    }

    async fn host_state_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let subscription = {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };

            match sessions_lock.subscribe_to_host_update(&token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("Note the host")),
            }
        };

        {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };
            sessions_lock.send_host_state();
        }

        match subscription.await {
            Ok(v) => v,
            Err(msg) => panic!("{msg}"),
        }
    }

    async fn is_player_handler(State(state): State<Arc<ServerState>>, jar: CookieJar) -> String {
        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };

        if let Some(old_cookie) = jar.get(Self::SESSION_COOKIE_NAME) {
            if let Some(token) = SessionToken::from_str(old_cookie.value_trimmed()) {
                if sessions_lock.is_player(&token) {
                    return "true".to_string();
                }
            }
        }

        "false".to_string()
    }

    async fn is_host_handler(State(state): State<Arc<ServerState>>, jar: CookieJar) -> String {
        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };

        if let Some(old_cookie) = jar.get(Self::SESSION_COOKIE_NAME) {
            if let Some(token) = SessionToken::from_str(old_cookie.value_trimmed()) {
                if sessions_lock.is_host(&token) {
                    return "true".to_string();
                }
            }
        }

        "false".to_string()
    }

    async fn input_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
        Json(playload): Json<DeserializableInputEvent>,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };
        let player = match sessions_lock.session_player_color(&token) {
            Ok(player) => player,
            Err(InvalidSessionError::DoesNotExist) => {
                return (
                    StatusCode::FORBIDDEN,
                    format!("Expired {}", Self::SESSION_COOKIE_NAME),
                );
            }
        };
        match <InputEvent as TryFrom<&DeserializableInputEvent>>::try_from(&playload) {
            Err(msg) => (StatusCode::BAD_REQUEST, msg),
            Ok(event) => {
                let player_event = PlayerInputEvent { player, event };
                let _ = state
                    .input_sender
                    .send(player_event)
                    .expect("Could not send input to main thread.");
                (StatusCode::ACCEPTED, "Input received.".to_owned())
            }
        }
    }

    async fn host_assignment_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, CookieJar, String) {
        let mut session_lock = state.sessions.lock().expect("Unable to lock host manager.");
        if session_lock.has_host() {
            return (
                StatusCode::FORBIDDEN,
                jar,
                format!("game host already exists"),
            );
        }

        let token = SessionToken::new();
        session_lock.set_host(token.clone());

        let mut session_cookie = Cookie::new(Self::SESSION_COOKIE_NAME, token.to_url_string());
        session_cookie.set_same_site(SameSite::Strict);
        session_cookie.set_path(Self::CONTROLLER_PATH);
        session_cookie.set_http_only(true);
        session_cookie.set_secure(true);
        session_cookie.set_max_age(Self::COOKIE_LIFETIME);
        let updated_jar = jar.add(session_cookie);

        println!("Host connected.");

        state
            .host_command_sender
            .send(PrivateHostCommand::HostConnected)
            .expect("Unable to send host connection signal.");

        (
            StatusCode::ACCEPTED,
            updated_jar,
            format!("you are the host"),
        )
    }

    fn validate_host(
        jar: &CookieJar,
        sessions: &mut SessionManager,
    ) -> Result<(), (StatusCode, String)> {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return Err(msg),
        };

        if !sessions.is_host(&token) {
            return Err((StatusCode::FORBIDDEN, format!("Not the host")));
        }

        Ok(())
    }

    async fn lobby_mode_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        if let Err(err) = Self::validate_host(&jar, &mut state.sessions.lock().unwrap()) {
            return err;
        }

        state
            .host_command_sender
            .send(PrivateHostCommand::PublicCommand(
                HostCommand::EnterLobbyMode,
            ))
            .expect("Could not send host command to main thread.");

        (StatusCode::ACCEPTED, format!("Entering lobby mode"))
    }

    async fn game_selector_mode_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        if let Err(err) = Self::validate_host(&jar, &mut state.sessions.lock().unwrap()) {
            return err;
        }

        state
            .host_command_sender
            .send(PrivateHostCommand::PublicCommand(
                HostCommand::EnterGameSelectionMode,
            ))
            .expect("Could not send host command to main thread.");

        (StatusCode::ACCEPTED, format!("Entering game selector mode"))
    }

    async fn player_assignment_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
        Json(payload): Json<DeserializablePlayerEntry>,
    ) -> (StatusCode, CookieJar, String) {
        let requested_color = match PlayerColor::try_from(&payload.color) {
            Ok(request) => request,
            Err(msg) => return (StatusCode::BAD_REQUEST, jar, msg),
        };

        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };

        if let Some(old_cookie) = jar.get(Self::SESSION_COOKIE_NAME) {
            if let Some(token) = SessionToken::from_str(old_cookie.value_trimmed()) {
                if sessions_lock.is_player(&token) {
                    return (
                        StatusCode::BAD_REQUEST,
                        jar,
                        NewSessionError::SessionAlreadyEstablished.to_string(),
                    );
                }

                // case where token corresponds to a host but not a player,
                // needs to be assigned a player
                if sessions_lock.is_host(&token) {
                    if let Err(e) = sessions_lock.new_host_player(requested_color) {
                        return (StatusCode::BAD_REQUEST, jar, e.to_string());
                    } else {
                        return (StatusCode::ACCEPTED, jar, "Player created".to_string());
                    }
                }
            }
        }

        let token = match sessions_lock.new_player(requested_color) {
            Ok(token) => token,
            Err(err) => {
                return (StatusCode::BAD_REQUEST, jar, err.to_string());
            }
        };

        let mut session_cookie = Cookie::new(Self::SESSION_COOKIE_NAME, token.to_url_string());
        session_cookie.set_same_site(SameSite::Strict);
        session_cookie.set_path(Self::CONTROLLER_PATH);
        session_cookie.set_http_only(true);
        session_cookie.set_secure(true);
        session_cookie.set_max_age(Self::COOKIE_LIFETIME);
        let updated_jar = jar.add(session_cookie);

        (
            StatusCode::ACCEPTED,
            updated_jar,
            "Player created".to_string(),
        )
    }

    /// creates and runs a new input server
    /// function does not return until a host has connected
    pub async fn run_new(address: String) -> Self {
        let host_addr: String = address;

        let (input_sender, input_receiver) = mpsc::channel();
        let (new_player_sender, new_player_receiver) = mpsc::channel();
        let (host_command_sender, host_command_receiver) = mpsc::channel();

        let sessions = Arc::new(Mutex::new(SessionManager::new(new_player_sender)));
        let state = ServerState {
            input_sender,
            host_command_sender,
            sessions: sessions.clone(),
        };

        let app = Router::new()
            .nest_service(Self::HOST_PATH, ServeDir::new(Self::STATIC_HOST_FILES_PATH))
            .nest_service(
                Self::CONTROLLER_PATH,
                ServeDir::new(Self::STATIC_CONTROLLER_FILES_PATH),
            )
            // controller apis
            .route(Self::IS_PLAYER_API_PATH, post(Self::is_player_handler))
            .route(
                Self::PLAYER_ASSIGNMENT_API_PATH,
                post(Self::player_assignment_handler),
            )
            .route(
                Self::CONTROLLER_EVENT_STREAM_PATH,
                post(Self::controller_event_handler),
            )
            .route(
                Self::CONTROLLER_STATE_PATH,
                post(Self::controller_state_handler),
            )
            .route(Self::INPUT_API_PATH, post(Self::input_handler))
            // host apis
            .route(Self::IS_HOST_API_PATH, post(Self::is_host_handler))
            .route(
                Self::HOST_ASSIGNMENT_PATH,
                post(Self::host_assignment_handler),
            )
            .route(Self::HOST_STATE_PATH, post(Self::host_state_handler))
            .route(Self::HOST_EVENT_STREAM_PATH, post(Self::host_event_handler))
            .route(
                Self::HOST_GAME_SELECTION_PATH,
                post(Self::game_selector_mode_handler),
            )
            .route(Self::HOST_LOBBY_PATH, post(Self::lobby_mode_handler))
            .with_state(state.into());

        let listener = tokio::net::TcpListener::bind(&host_addr)
            .await
            .expect(&format!("Failed to bind to port: {host_addr}."));
        let service_thread = tokio::spawn(async move { axum::serve(listener, app).await });
        println!("Server started at {host_addr}.");

        println!("Awaiting host connection at {host_addr}{}", Self::HOST_PATH);
        for cmd in host_command_receiver.iter() {
            if cmd == PrivateHostCommand::HostConnected {
                break;
            }
        }

        let out = Self {
            ip_addr: host_addr,
            sessions,
            input_receiver,
            player_event_receiver: new_player_receiver,
            host_command_receiver,
            service_thread,
        };

        out.allow_new_players();

        out
    }

    /// gets the address of the server
    pub fn address(&self) -> &str {
        &self.ip_addr
    }

    /// blocks new players from joining
    pub fn block_new_players(&self) {
        self.sessions
            .lock()
            .expect("Unable to block new players")
            .block_joins();
    }

    /// allows new players to join
    /// (enabled upon server creation)
    pub fn allow_new_players(&self) {
        self.sessions
            .lock()
            .expect("Unable to allow new players")
            .allow_joins();
    }

    /// kicks the given player
    pub fn kick_player(&self, color: PlayerColor) {
        self.sessions
            .lock()
            .expect("Unable to kick player")
            .kick_player(color);
    }

    pub fn poll_inputs(&self) -> impl Iterator<Item = PlayerInputEvent> {
        self.input_receiver.try_iter()
    }

    pub fn poll_player_events(&self) -> impl Iterator<Item = PlayerEvent> {
        self.player_event_receiver.try_iter()
    }

    pub fn poll_host_commands(&self) -> impl Iterator<Item = HostCommand> {
        self.host_command_receiver
            .try_iter()
            .filter_map(|cmd| match cmd {
                PrivateHostCommand::PublicCommand(cmd) => Some(cmd),
                _ => None,
            })
    }

    pub fn abort(self) {
        self.service_thread.abort();
    }
}
