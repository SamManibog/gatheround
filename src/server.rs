use std::{
    collections::HashMap,
    fmt::Display,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
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
use serde::Deserialize;
use time::Duration;
use tokio::{self, task::JoinHandle};
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

#[derive(Debug, Default)]
struct HostManager {
    host: Option<SessionToken>,
}

impl HostManager {
    pub fn has_host(&self) -> bool {
        self.host.is_some()
    }

    pub fn set_host(&mut self, token: SessionToken) {
        self.host = Some(token);
    }

    pub fn is_host(&self, token: &SessionToken) -> bool {
        if let Some(host_token) = &self.host {
            host_token == token
        } else {
            false
        }
    }
}

#[derive(Debug)]
struct SessionManager {
    new_player_sender: Sender<PlayerColor>,
    kick_player_reciever: Receiver<PlayerColor>,
    token_player_map: HashMap<SessionToken, PlayerColor>,
    used_players: DenseComponentManager<PlayerColor, SessionToken>,
    allow_joins: Arc<AtomicBool>,
}

impl SessionManager {
    pub fn new(
        new_player_sender: Sender<PlayerColor>,
        kick_player_reciever: Receiver<PlayerColor>,
        allow_joins: Arc<AtomicBool>,
    ) -> Self {
        Self {
            new_player_sender,
            kick_player_reciever,
            token_player_map: Default::default(),
            used_players: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            allow_joins,
        }
    }

    fn handle_kicks(&mut self) {
        for player in self.kick_player_reciever.try_iter() {
            if let Some(token) = self.used_players.remove(player) {
                self.token_player_map.remove(&token);
            }
        }
    }

    /// creates a new player
    pub fn new_player(&mut self, color: PlayerColor) -> Result<SessionToken, NewSessionError> {
        self.handle_kicks();

        if !self.allow_joins.load(Ordering::Relaxed) {
            return Err(NewSessionError::NewJoinsBlocked);
        }

        if self.used_players.len() >= PlayerColor::COLOR_COUNT {
            return Err(NewSessionError::GameFull);
        }

        if self.used_players.contains(color) {
            return Err(NewSessionError::PlayerAlreadyUsed);
        }

        let token = {
            let mut token = SessionToken::new();
            while self.token_player_map.contains_key(&token) {
                token = SessionToken::new();
            }
            token
        };

        self.token_player_map.insert(token.clone(), color);
        self.used_players.insert(color, token.clone());
        self.new_player_sender
            .send(color)
            .expect("Could not send new player to main thread.");

        Ok(token.clone())
    }

    /// gets the player color associated with the given session
    pub fn session_player_color(
        &mut self,
        token: &SessionToken,
    ) -> Result<PlayerColor, InvalidSessionError> {
        self.handle_kicks();

        match self.token_player_map.get(token) {
            Some(color) => Ok(*color),
            None => Err(InvalidSessionError::DoesNotExist),
        }
    }
}

#[derive(Debug)]
pub struct ServerState {
    input_sender: Sender<PlayerInputEvent>,
    host_command_sender: Sender<PrivateHostCommand>,
    sessions: Mutex<SessionManager>,
    host: Mutex<HostManager>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeserializablePlayerEntry {
    /// the player's name
    pub color: String,
}

#[derive(Debug)]
pub struct GameServer {
    input_receiver: Receiver<PlayerInputEvent>,
    new_player_receiver: Receiver<PlayerColor>,
    host_command_receiver: Receiver<PrivateHostCommand>,
    kick_player_sender: Sender<PlayerColor>,

    service_thread: JoinHandle<Result<(), tokio::io::Error>>,
    allow_joins: Arc<AtomicBool>,
    ip_addr: String,
}

impl GameServer {
    pub const HOST_PATH: &str = "/gamehost";
    pub const CLAIM_HOST_PATH: &str = "/gamehost/claim";
    pub const HOST_LOBBY_PATH: &str = "/gamehost/start-lobby";
    pub const HOST_GAME_SELECTION_PATH: &str = "/gamehost/select-game";

    pub const CONTROLLER_PATH: &str = "/games";
    pub const INPUT_API_PATH: &str = "/games/input";
    pub const PLAYER_ASSIGNMENT_API_PATH: &str = "/games/get-session";
    pub const HAS_SESSION_API_PATH: &str = "/games/has-session";

    pub const STATIC_CONTROLLER_FILES_PATH: &str = "./static/controller";
    pub const STATIC_HOST_FILES_PATH: &str = "./static/host";

    pub const SESSION_COOKIE_NAME: &str = "session_id";
    pub const HOST_COOKIE_NAME: &str = "host_id";

    pub const COOKIE_LIFETIME: Duration = Duration::days(7);

    async fn has_session_handler(State(state): State<Arc<ServerState>>, jar: CookieJar) -> String {
        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };

        if let Some(old_cookie) = jar.get(Self::SESSION_COOKIE_NAME) {
            if let Some(token) = SessionToken::from_str(old_cookie.value_trimmed()) {
                if sessions_lock.session_player_color(&token).is_ok() {
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
        let token_string = match jar.get(Self::SESSION_COOKIE_NAME) {
            Some(cookie) => cookie,
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    format!("Missing {}", Self::SESSION_COOKIE_NAME),
                );
            }
        }
        .value_trimmed();
        let token = match SessionToken::from_str(token_string) {
            Some(token) => token,
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    format!("Invalid {}", Self::SESSION_COOKIE_NAME),
                );
            }
        };

        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };
        let player = match sessions_lock.session_player_color(&token) {
            Ok(player) => player,
            Err(InvalidSessionError::DoesNotExist) => {
                return (
                    StatusCode::UNAUTHORIZED,
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

    async fn claim_host_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, CookieJar, String) {
        println!("hosting");
        let mut host_lock = state.host.lock().expect("Unable to lock host manager.");
        if host_lock.has_host() {
            return (
                StatusCode::FORBIDDEN,
                jar,
                format!("game host already exists"),
            );
        }

        let token = SessionToken::new();
        host_lock.set_host(token.clone());

        let mut host_cookie = Cookie::new(Self::HOST_COOKIE_NAME, token.to_url_string());
        host_cookie.set_same_site(SameSite::Strict);
        host_cookie.set_path(Self::HOST_PATH);
        host_cookie.set_http_only(true);
        host_cookie.set_secure(true);
        host_cookie.set_max_age(Self::COOKIE_LIFETIME);
        let updated_jar = jar.add(host_cookie);

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

    fn validate_host(jar: &CookieJar, host: &mut HostManager) -> Result<(), (StatusCode, String)> {
        let token_string = match jar.get(Self::HOST_COOKIE_NAME) {
            Some(cookie) => cookie,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    format!("Missing {}", Self::HOST_COOKIE_NAME),
                ));
            }
        }
        .value_trimmed();
        let token = match SessionToken::from_str(token_string) {
            Some(token) => token,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    format!("Invalid {}", Self::HOST_COOKIE_NAME),
                ));
            }
        };

        if !host.is_host(&token) {
            return Err((StatusCode::FORBIDDEN, format!("Not the host")));
        }

        Ok(())
    }

    async fn lobby_mode_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        if let Err(err) = Self::validate_host(&jar, &mut state.host.lock().unwrap()) {
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
        if let Err(err) = Self::validate_host(&jar, &mut state.host.lock().unwrap()) {
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
                if sessions_lock.session_player_color(&token).is_ok() {
                    return (
                        StatusCode::BAD_REQUEST,
                        jar,
                        NewSessionError::SessionAlreadyEstablished.to_string(),
                    );
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
        let (kick_player_sender, kick_player_receiver) = mpsc::channel();
        let allow_joins = Arc::new(AtomicBool::new(false));

        let state = ServerState {
            input_sender,
            host: Mutex::new(HostManager::default()),
            host_command_sender,
            sessions: Mutex::new(SessionManager::new(
                new_player_sender,
                kick_player_receiver,
                allow_joins.clone(),
            )),
        };

        let app = Router::new()
            .nest_service(Self::HOST_PATH, ServeDir::new(Self::STATIC_HOST_FILES_PATH))
            .nest_service(
                Self::CONTROLLER_PATH,
                ServeDir::new(Self::STATIC_CONTROLLER_FILES_PATH),
            )
            .route(Self::INPUT_API_PATH, post(Self::input_handler))
            .route(Self::HAS_SESSION_API_PATH, post(Self::has_session_handler))
            .route(
                Self::HOST_GAME_SELECTION_PATH,
                post(Self::game_selector_mode_handler),
            )
            .route(Self::HOST_LOBBY_PATH, post(Self::lobby_mode_handler))
            .route(Self::CLAIM_HOST_PATH, post(Self::claim_host_handler))
            .route(
                Self::PLAYER_ASSIGNMENT_API_PATH,
                post(Self::player_assignment_handler),
            )
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

        Self {
            ip_addr: host_addr,
            allow_joins,
            input_receiver,
            new_player_receiver,
            host_command_receiver,
            kick_player_sender,
            service_thread,
        }
    }

    /// gets the address of the server
    pub fn address(&self) -> &str {
        &self.ip_addr
    }

    /// blocks new players from joining
    pub fn block_new_players(&self) {
        self.allow_joins.store(false, Ordering::Relaxed);
    }

    /// allows new players to join
    /// (enabled upon server creation)
    pub fn allow_new_players(&self) {
        self.allow_joins.store(true, Ordering::Relaxed);
    }

    /// sends a signal to kick the given player
    pub fn kick_player(&self, player: PlayerColor) {
        self.kick_player_sender
            .send(player)
            .expect("Unable to kick player");
    }

    pub fn poll_inputs(&self) -> impl Iterator<Item = PlayerInputEvent> {
        self.input_receiver.try_iter()
    }

    pub fn poll_new_players(&self) -> impl Iterator<Item = PlayerColor> {
        self.new_player_receiver.try_iter()
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
