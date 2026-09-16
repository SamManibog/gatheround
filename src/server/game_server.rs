use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use axum_extra::extract::{CookieJar, cookie::{Cookie, SameSite}};
use serde::Deserialize;
use time::Duration;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

use crate::{input::{DeserializableInputEvent, InputEvent, PlayerInputEvent}, resources::PlayerColor, server::{InvalidSessionError, NewSessionError, SessionManager, SessionToken}};

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
