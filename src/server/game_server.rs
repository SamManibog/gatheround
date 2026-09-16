use std::{
    hint::cold_path,
    net::SocketAddr,
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
use axum_server::tls_rustls::RustlsConfig;
use serde::Deserialize;
use time::Duration;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

use crate::{
    board::players::PlayerColor,
    input::{DeserializableInputEvent, InputEvent, PlayerInputEvent},
    server::{InvalidSessionError, SessionManager, SessionToken, SubscriptionKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateHostCommand {
    HostConnected,
    PublicCommand(HostCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCommand {
    EnterLobby,
    EnterGameSelector,
    KickPlayer(PlayerColor),
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

#[derive(Debug, Clone, Deserialize)]
struct DeserializableHostEvent {
    pub start_lobby: Option<bool>,
    pub start_game_selector: Option<bool>,
    pub kick_player: Option<String>,
}

impl TryInto<Vec<HostCommand>> for &DeserializableHostEvent {
    type Error = String;

    fn try_into(self) -> Result<Vec<HostCommand>, Self::Error> {
        let mut vec = Vec::new();

        if let Some(true) = self.start_lobby {
            vec.push(HostCommand::EnterLobby);
        }

        if let Some(true) = self.start_game_selector {
            vec.push(HostCommand::EnterGameSelector);
        }

        if let Some(color_str) = &self.kick_player {
            if let Some(color) = PlayerColor::from_name(&color_str) {
                vec.push(HostCommand::KickPlayer(color));
            } else {
                return Err(format!("invalid color: {color_str}"));
            }
        }

        Ok(vec)
    }
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
    pub const PLAYER_PATH: &str = "/games";

    pub const SESSION_ASSIGNMENT_PATH: &str = "/games/join";
    pub const PLAYER_ASSIGNMENT_PATH: &str = "/games/claim-player";
    pub const HOST_ASSIGNMENT_PATH: &str = "/games/claim-host";

    pub const JOIN_EVENT_STREAM_PATH: &str = "/games/join-events";
    pub const PLAYER_EVENT_STREAM_PATH: &str = "/games/player-events";
    pub const HOST_EVENT_STREAM_PATH: &str = "/games/host-events";

    pub const JOIN_STATE_PATH: &str = "/games/join-state";
    pub const PLAYER_STATE_PATH: &str = "/games/player-state";
    pub const HOST_STATE_PATH: &str = "/games/host-state";

    pub const PLAYER_API_PATH: &str = "/games/player-api";
    pub const HOST_API_PATH: &str = "/games/host-api";

    pub const STATIC_PLAYER_FILES_PATH: &str = "./static/controller";
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

    async fn generic_event_handler(
        state: Arc<ServerState>,
        jar: &CookieJar,
        subscription_kind: SubscriptionKind,
        failure_msg: &str,
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

            match sessions_lock.subscribe_to_update(subscription_kind, &token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("{failure_msg}")),
            }
        };

        match subscription.await {
            Ok(v) => v,
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("subscription could not be resolved"),
            ),
        }
    }

    async fn join_event_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_event_handler(
            state,
            &jar,
            SubscriptionKind::NonPlayer,
            "session does not exist",
        )
        .await
    }

    async fn player_event_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_event_handler(
            state,
            &jar,
            SubscriptionKind::PlayerController,
            "not a player",
        )
        .await
    }

    async fn host_event_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_event_handler(state, &jar, SubscriptionKind::Host, "not the host").await
    }

    async fn generic_state_handler(
        state: Arc<ServerState>,
        jar: &CookieJar,
        subscription_kind: SubscriptionKind,
        failure_msg: &str,
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

            match sessions_lock.subscribe_to_update(subscription_kind, &token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("{failure_msg}")),
            }
        };

        {
            let mut sessions_lock = match state.sessions.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };
            sessions_lock.send_state(subscription_kind, &token);
        }

        match subscription.await {
            Ok(v) => v,
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("subscription could not be resolved"),
            ),
        }
    }

    async fn join_state_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_state_handler(
            state,
            &jar,
            SubscriptionKind::NonPlayer,
            "session does not exist",
        )
        .await
    }

    async fn player_state_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_state_handler(
            state,
            &jar,
            SubscriptionKind::PlayerController,
            "not a player",
        )
        .await
    }

    async fn host_state_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_state_handler(state, &jar, SubscriptionKind::Host, "not the host").await
    }

    async fn player_api_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
        Json(payload): Json<DeserializableInputEvent>,
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

        match <InputEvent as TryFrom<&DeserializableInputEvent>>::try_from(&payload) {
            Err(msg) => (StatusCode::BAD_REQUEST, msg),
            Ok(event) => {
                let player_event = PlayerInputEvent { player, event };
                let _ = state
                    .input_sender
                    .send(player_event)
                    .expect("Could not send input to main thread.");
                (StatusCode::ACCEPTED, format!("input received"))
            }
        }
    }

    async fn host_api_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
        Json(payload): Json<DeserializableHostEvent>,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };

        if !sessions_lock.is_host(&token) {
            return (StatusCode::FORBIDDEN, format!("not the host"));
        }

        match <&DeserializableHostEvent as TryInto<Vec<HostCommand>>>::try_into(&payload) {
            Err(msg) => (StatusCode::BAD_REQUEST, msg),
            Ok(events) => {
                for event in events {
                    state
                        .host_command_sender
                        .send(PrivateHostCommand::PublicCommand(event))
                        .expect("Could not send host command to main thread.");
                }
                (StatusCode::ACCEPTED, format!("command received"))
            }
        }
    }

    async fn session_assignment_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, CookieJar, String) {
        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{msg}"),
        };

        // if we already have a valid token, then don't modify cookies
        if let Ok(token) = Self::parse_session_cookie(&jar) {
            if sessions_lock.is_player(&token) {
                return (StatusCode::ACCEPTED, jar, format!("is player"));
            }
            if sessions_lock.is_session(&token) {
                return (StatusCode::ACCEPTED, jar, format!("has session"));
            }
        }

        // if we have an invalid token, modify cookies
        match sessions_lock.new_session() {
            Some(token) => {
                let mut session_cookie =
                    Cookie::new(Self::SESSION_COOKIE_NAME, token.to_url_string());
                session_cookie.set_same_site(SameSite::Strict);
                session_cookie.set_path(Self::PLAYER_PATH);
                session_cookie.set_http_only(true);
                session_cookie.set_secure(true);
                session_cookie.set_max_age(Self::COOKIE_LIFETIME);
                let updated_jar = jar.add(session_cookie);

                (StatusCode::CREATED, updated_jar, format!("new session"))
            }

            None => (StatusCode::LOCKED, jar, format!("joins disabled")),
        }
    }

    async fn player_assignment_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
        Json(payload): Json<DeserializablePlayerEntry>,
    ) -> (StatusCode, String) {
        let requested_color = match PlayerColor::try_from(&payload.color) {
            Ok(request) => request,
            Err(msg) => return (StatusCode::BAD_REQUEST, msg),
        };

        let token = match Self::parse_session_cookie(&jar) {
            Ok(token) => token,
            Err((code, text)) => return (code, text),
        };

        let mut sessions_lock = match state.sessions.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{msg}"),
        };

        match sessions_lock.promote_to_player(&token, requested_color) {
            Ok(()) => (StatusCode::ACCEPTED, format!("player assigned")),
            Err(msg) => (msg.status_code(), format!("{msg}")),
        }
    }

    async fn host_assignment_handler(
        State(state): State<Arc<ServerState>>,
        jar: CookieJar,
    ) -> (StatusCode, CookieJar, String) {
        let mut session_lock = state.sessions.lock().expect("Unable to lock host manager.");

        // host reconnection
        if let Ok(token) = Self::parse_session_cookie(&jar) {
            if session_lock.is_host(&token) {
                return (StatusCode::ACCEPTED, jar, format!("is host"));
            }
        }

        // first time connection only
        if session_lock.has_host() {
            return (
                StatusCode::FORBIDDEN,
                jar,
                format!("game host already exists"),
            );
        }

        // has session, but not host
        if let Ok(token) = Self::parse_session_cookie(&jar) {
            session_lock.set_host(token.clone());

            state
                .host_command_sender
                .send(PrivateHostCommand::HostConnected)
                .expect("Unable to send host connection signal.");

            return (StatusCode::ACCEPTED, jar, format!("new host"));
        }

        // no session, needs to be host now
        let token = SessionToken::new();
        session_lock.set_host(token.clone());

        let mut session_cookie = Cookie::new(Self::SESSION_COOKIE_NAME, token.to_url_string());
        session_cookie.set_same_site(SameSite::Strict);
        session_cookie.set_path(Self::PLAYER_PATH);
        session_cookie.set_http_only(true);
        session_cookie.set_secure(true);
        session_cookie.set_max_age(Self::COOKIE_LIFETIME);
        let updated_jar = jar.add(session_cookie);

        println!("Host connected.");

        state
            .host_command_sender
            .send(PrivateHostCommand::HostConnected)
            .expect("Unable to send host connection signal.");

        (StatusCode::ACCEPTED, updated_jar, format!("new host"))
    }

    /// creates and runs a new input server
    /// function does not return until a host has connected
    pub async fn run_new(address: String) -> Self {
        let socket_addr: SocketAddr = address.parse().expect("Unable to parse address");

        let certified_key =
            rcgen::generate_simple_self_signed(vec![format!("{}{}", address, Self::PLAYER_PATH)])
                .expect("Unable to generate https certificate.");

        let tls_config = RustlsConfig::from_pem(
            certified_key.cert.pem().into_bytes(),
            certified_key.signing_key.serialize_pem().into_bytes(),
        )
        .await
        .expect("Unable to create TLS configuration");

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
                Self::PLAYER_PATH,
                ServeDir::new(Self::STATIC_PLAYER_FILES_PATH),
            )
            // assignment apis
            .route(
                Self::SESSION_ASSIGNMENT_PATH,
                post(Self::session_assignment_handler),
            )
            .route(
                Self::PLAYER_ASSIGNMENT_PATH,
                post(Self::player_assignment_handler),
            )
            .route(
                Self::HOST_ASSIGNMENT_PATH,
                post(Self::host_assignment_handler),
            )
            // event stream apis
            .route(Self::JOIN_EVENT_STREAM_PATH, post(Self::join_event_handler))
            .route(
                Self::PLAYER_EVENT_STREAM_PATH,
                post(Self::player_event_handler),
            )
            .route(Self::HOST_EVENT_STREAM_PATH, post(Self::host_event_handler))
            // state api
            .route(Self::JOIN_STATE_PATH, post(Self::join_state_handler))
            .route(Self::PLAYER_STATE_PATH, post(Self::player_state_handler))
            .route(Self::HOST_STATE_PATH, post(Self::host_state_handler))
            // main apis
            .route(Self::PLAYER_API_PATH, post(Self::player_api_handler))
            .route(Self::HOST_API_PATH, post(Self::host_api_handler))
            .with_state(state.into());

        let service_thread = tokio::spawn(async move {
            axum_server::bind_rustls(socket_addr, tls_config)
                .serve(app.into_make_service())
                .await
        });
        println!("Server started at https://{address}.");

        println!("Awaiting host connection at https://{address}{}", Self::HOST_PATH);
        for cmd in host_command_receiver.iter() {
            if cmd == PrivateHostCommand::HostConnected {
                break;
            }
        }

        let out = Self {
            ip_addr: address,
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
                PrivateHostCommand::PublicCommand(cmd) => {
                    cold_path();
                    Some(cmd)
                }
                _ => None,
            })
    }

    pub fn abort(self) {
        self.service_thread.abort();
    }
}
