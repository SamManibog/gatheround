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
use serde::Deserialize;
use time::Duration;
use tokio::{self, task::JoinHandle};
use tower_http::services::ServeDir;

use crate::{
    ecs::DenseSet,
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

enum NewSessionError {
    GameFull,
    PlayerAlreadyUsed,
    SessionAlreadyEstablished,
}

impl Display for NewSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::GameFull => "game full",
            Self::PlayerAlreadyUsed => "color taken",
            Self::SessionAlreadyEstablished => "session exists",
        };
        write!(f, "{}", message)
    }
}

#[derive(Debug)]
struct SessionManager {
    new_player_sender: Sender<PlayerColor>,
    token_player_map: HashMap<Box<SessionToken>, PlayerColor>,
    used_players: DenseSet<PlayerColor>,
}

impl SessionManager {
    pub fn new(new_player_sender: Sender<PlayerColor>) -> Self {
        Self {
            new_player_sender,
            token_player_map: Default::default(),
            used_players: DenseSet::with_capacity(PlayerColor::COLOR_COUNT),
        }
    }

    /// creates a new player
    pub fn new_player(&mut self, color: PlayerColor) -> Result<SessionToken, NewSessionError> {
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

        self.token_player_map.insert(Box::new(token.clone()), color);
        self.used_players.insert(color);
        self.new_player_sender.send(color).expect("Could not send new player to main thread.");
        println!("sending new player");

        Ok(token)
    }

    /// gets the player color associated with the given session
    pub fn session_player_color(
        &self,
        token: &SessionToken,
    ) -> Result<PlayerColor, InvalidSessionError> {
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

pub struct GameServer {
    input_receiver: Receiver<PlayerInputEvent>,
    new_player_receiver: Receiver<PlayerColor>,
    service_thread: JoinHandle<Result<(), tokio::io::Error>>,
}

impl GameServer {
    //pub const HOST_ADDR: &str = "192.168.137.1:443";
    pub const HOST_ADDR: &str = "127.0.0.1:8080";

    pub const GATHEROUND_PATH: &str = "/gatheround";
    pub const INPUT_API_PATH: &str = "/gatheround/input";
    pub const PLAYER_ASSIGNMENT_API_PATH: &str = "/gatheround/get-session";
    pub const HAS_SESSION_API_PATH: &str = "/gatheround/has-session";

    pub const STATIC_FILES_PATH: &str = "./static";

    pub const SESSION_COOKIE_NAME: &str = "session_id";

    pub const COOKIE_LIFETIME: Duration = Duration::days(7);

    async fn has_session_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> String {
        let sessions_lock = match state.sessions.lock() {
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

        let sessions_lock = match state.sessions.lock() {
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
        session_cookie.set_path(Self::GATHEROUND_PATH);
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
    pub async fn run_new() -> Self {
        let host_addr = Self::HOST_ADDR;

        let (input_sender, input_receiver) = mpsc::channel();
        let (new_player_sender, new_player_receiver) = mpsc::channel();

        let state = ServerState {
            input_sender,
            sessions: Mutex::new(SessionManager::new(new_player_sender)),
        };

        let app = Router::new()
            .nest_service(
                Self::GATHEROUND_PATH,
                ServeDir::new(Self::STATIC_FILES_PATH),
            )
            .route(Self::INPUT_API_PATH, post(Self::input_handler))
            .route(Self::HAS_SESSION_API_PATH, post(Self::has_session_handler))
            .route(
                Self::PLAYER_ASSIGNMENT_API_PATH,
                post(Self::player_assignment_handler),
            )
            .with_state(state.into());

        let listener = tokio::net::TcpListener::bind(host_addr)
            .await
            .expect(&format!("Failed to bind to port: {host_addr}."));
        let service_thread = tokio::spawn(async move { axum::serve(listener, app).await });
        println!("Server started at {host_addr}.");

        Self {
            input_receiver,
            new_player_receiver,
            service_thread,
        }
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
