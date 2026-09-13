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
        println!("sending new player");

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
    sessions: Mutex<SessionManager>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeserializablePlayerJoinRequest {
    /// the player's name
    pub color: String,
}

#[derive(Debug)]
pub struct GameServer {
    input_receiver: Receiver<PlayerInputEvent>,
    new_player_receiver: Receiver<PlayerColor>,
    kick_player_sender: Sender<PlayerColor>,
    service_thread: JoinHandle<Result<(), tokio::io::Error>>,
    allow_joins: Arc<AtomicBool>,
    host_addr: String,
}

impl GameServer {
    //pub const HOST_ADDR: &str = "192.168.137.1:443";
    pub const HOST_ADDR: &str = "127.0.0.1:8080";

    pub const MAIN_PATH: &str = "/gatheround";
    pub const INPUT_API_PATH: &str = "/gatheround/input";
    pub const PLAYER_ASSIGNMENT_API_PATH: &str = "/gatheround/get-session";
    pub const HAS_SESSION_API_PATH: &str = "/gatheround/has-session";

    pub const STATIC_FILES_PATH: &str = "./static";

    pub const SESSION_COOKIE_NAME: &str = "session_id";

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
            None => return (StatusCode::UNAUTHORIZED, "Missing session_id".to_owned()),
        }
        .value_trimmed();
        let token = match SessionToken::from_str(token_string) {
            Some(token) => token,
            None => return (StatusCode::UNAUTHORIZED, "Invalid session_id".to_owned()),
        };

        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };
        let player = match sessions_lock.session_player_color(&token) {
            Ok(player) => player,
            Err(InvalidSessionError::DoesNotExist) => {
                return (StatusCode::UNAUTHORIZED, "Expired session_id".to_owned());
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

    async fn player_assignment_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
        Json(payload): Json<DeserializablePlayerJoinRequest>,
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
        session_cookie.set_path(Self::MAIN_PATH);
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
    pub async fn run_new(address: String) -> Self {
        let host_addr: String = address;

        let (input_sender, input_receiver) = mpsc::channel();
        let (new_player_sender, new_player_receiver) = mpsc::channel();
        let (kick_player_sender, kick_player_receiver) = mpsc::channel();
        let allow_joins = Arc::new(AtomicBool::new(true));

        let state = ServerState {
            input_sender,
            sessions: Mutex::new(SessionManager::new(
                new_player_sender,
                kick_player_receiver,
                allow_joins.clone(),
            )),
        };

        let app = Router::new()
            .nest_service(
                Self::MAIN_PATH,
                ServeDir::new(Self::STATIC_FILES_PATH),
            )
            .route(Self::INPUT_API_PATH, post(Self::input_handler))
            .route(Self::HAS_SESSION_API_PATH, post(Self::has_session_handler))
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

        Self {
            host_addr,
            allow_joins,
            input_receiver,
            new_player_receiver,
            kick_player_sender,
            service_thread,
        }
    }

    /// gets the address of the server
    pub fn address(&self) -> &str {
        &self.host_addr
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

    pub fn abort(self) {
        self.service_thread.abort();
    }
}
