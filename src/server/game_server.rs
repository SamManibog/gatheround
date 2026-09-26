use std::{
    io::Write,
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::AtomicBool,
        mpsc::{self, Receiver},
    },
    thread::{self},
};

use axum::{
    Json, Router,
    extract::{State, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
    routing::post,
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use axum_server::{Handle, tls_rustls::RustlsConfig};
use serde::{Deserialize, Serialize};
use time::Duration;
use tower_http::services::ServeDir;

use crate::{
    board::players::PlayerColor,
    input::PlayerInputEvent,
    server::{
        HostCandidateResponse, HostJoinCode, ServerEvent, ServerState, SessionKind, SessionToken,
    },
};

#[derive(Debug, Serialize)]
struct SerializableSessionType {
    pub is_player: bool,
    pub is_host: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct DeserializableHostEvent {
    pub start_lobby: Option<bool>,
    pub start_game_selector: Option<bool>,
    pub kick_player: Option<String>,
}

impl TryInto<Vec<ServerEvent>> for &DeserializableHostEvent {
    type Error = String;

    fn try_into(self) -> Result<Vec<ServerEvent>, Self::Error> {
        let mut vec = Vec::new();

        if let Some(true) = self.start_lobby {
            vec.push(ServerEvent::EnterLobby);
        }

        if let Some(true) = self.start_game_selector {
            vec.push(ServerEvent::EnterGameSelector);
        }

        if let Some(color_str) = &self.kick_player {
            if let Some(color) = PlayerColor::from_name(&color_str) {
                vec.push(ServerEvent::TryKickPlayer(color));
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
    event_receiver: Receiver<ServerEvent>,

    state: Arc<Mutex<ServerState>>,

    server_handle: Handle<SocketAddr>,
    ip_addr: String,
}

impl GameServer {
    pub const HOST_PATH: &str = "/games/host";
    pub const PLAYER_PATH: &str = "/games";

    pub const SESSION_ASSIGNMENT_PATH: &str = "/games/join";
    pub const NEW_HOST_CANDIDATE_PATH: &str = "/games/claim-host";
    pub const HOST_ACCEPTANCE_PATH: &str = "/games/is-host";

    pub const JOIN_EVENT_STREAM_PATH: &str = "/games/join-events";
    pub const PLAYER_EVENT_STREAM_PATH: &str = "/games/player-events";
    pub const HOST_EVENT_STREAM_PATH: &str = "/games/host-events";

    pub const JOIN_STATE_PATH: &str = "/games/join-state";
    pub const PLAYER_STATE_PATH: &str = "/games/player-state";
    pub const HOST_STATE_PATH: &str = "/games/host-state";

    pub const PLAYER_WEBSOCKET_PATH: &str = "/games/player-ws";
    pub const HOST_API_PATH: &str = "/games/host-api";

    pub const STATIC_PLAYER_FILES_PATH: &str = "./static/controller";
    pub const STATIC_HOST_FILES_PATH: &str = "./static/host";

    pub const SESSION_COOKIE_NAME: &str = "session_id";
    pub const COOKIE_LIFETIME: Duration = Duration::days(7);

    const OPEN_SERVER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

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
        state: Arc<Mutex<ServerState>>,
        jar: &CookieJar,
        subscription_kind: SessionKind,
        failure_msg: &str,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let subscription = {
            let mut state_lock = match state.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };

            match state_lock.subscribe_to_update(subscription_kind, &token) {
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
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_event_handler(
            state,
            &jar,
            SessionKind::NonPlayer,
            "session does not exist",
        )
        .await
    }

    async fn player_event_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_event_handler(state, &jar, SessionKind::PlayerController, "not a player")
            .await
    }

    async fn host_event_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_event_handler(state, &jar, SessionKind::Host, "not the host").await
    }

    async fn generic_state_handler(
        state: Arc<Mutex<ServerState>>,
        jar: &CookieJar,
        session_kind: SessionKind,
        failure_msg: &str,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let subscription = {
            let mut state_lock = match state.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };

            match state_lock.subscribe_to_update(session_kind, &token) {
                Some(rx) => rx,
                None => return (StatusCode::FORBIDDEN, format!("{failure_msg}")),
            }
        };

        {
            let mut state_lock = match state.lock() {
                Ok(lock) => lock,
                Err(msg) => panic!("{msg}"),
            };
            state_lock.send_state(session_kind, &token);
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
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_state_handler(
            state,
            &jar,
            SessionKind::NonPlayer,
            "session does not exist",
        )
        .await
    }

    async fn player_state_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_state_handler(state, &jar, SessionKind::PlayerController, "not a player")
            .await
    }

    async fn host_state_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        Self::generic_state_handler(state, &jar, SessionKind::Host, "not the host").await
    }

    async fn host_api_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
        Json(payload): Json<DeserializableHostEvent>,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(t) => t,
            Err(msg) => return msg,
        };

        let cmds = match <&DeserializableHostEvent as TryInto<Vec<ServerEvent>>>::try_into(&payload)
        {
            Err(msg) => return (StatusCode::BAD_REQUEST, msg),
            Ok(c) => c,
        };

        let mut state_lock = match state.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{}", msg),
        };

        if state_lock.handle_host_commands(&token, cmds) {
            (StatusCode::ACCEPTED, format!("command received"))
        } else {
            (StatusCode::FORBIDDEN, format!("not the host"))
        }
    }

    async fn session_assignment_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, CookieJar, String) {
        let mut state_lock = match state.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{msg}"),
        };

        // if we already have a valid token, then don't modify cookies
        if let Ok(token) = Self::parse_session_cookie(&jar) {
            if state_lock.is_session(&token) {
                return (
                    StatusCode::ACCEPTED,
                    jar,
                    serde_json::to_string(&SerializableSessionType {
                        is_player: state_lock.is_player(&token),
                        is_host: state_lock.is_host(&token),
                    })
                    .unwrap(),
                );
            }
        }

        // if we have an invalid token, try to create a new session
        match state_lock.new_session() {
            Some(token) => {
                let mut session_cookie =
                    Cookie::new(Self::SESSION_COOKIE_NAME, token.to_url_string());
                session_cookie.set_same_site(SameSite::Strict);
                session_cookie.set_path("/");
                session_cookie.set_max_age(Self::COOKIE_LIFETIME);
                let updated_jar = jar.add(session_cookie);

                (
                    StatusCode::CREATED,
                    updated_jar,
                    serde_json::to_string(&SerializableSessionType {
                        is_player: false,
                        is_host: false,
                    })
                    .unwrap(),
                )
            }

            None => (StatusCode::LOCKED, jar, format!("joins disabled")),
        }
    }

    async fn player_websocket_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        ws: WebSocketUpgrade,
    ) -> Response {
        let mut state_lock = match state.lock() {
            Ok(lock) => lock,
            Err(msg) => panic!("{msg}"),
        };
        state_lock.establish_player_websocket(ws, state.clone())
    }

    async fn new_host_candidate_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(token) => token,
            Err(err) => return err,
        };

        let mut session_lock = state.lock().expect("Unable to lock host manager.");
        match session_lock.make_host_candidate(token) {
            Ok(token) => (StatusCode::ACCEPTED, token.to_string()),
            Err(err) => (StatusCode::FORBIDDEN, err.to_string()),
        }
    }

    async fn host_acceptance_handler(
        State(state): State<Arc<Mutex<ServerState>>>,
        jar: CookieJar,
    ) -> (StatusCode, String) {
        let token = match Self::parse_session_cookie(&jar) {
            Ok(token) => token,
            Err(err) => return err,
        };

        let future_result = {
            let mut session_lock = state.lock().expect("Unable to lock host manager.");

            if session_lock.is_host(&token) {
                return (StatusCode::ACCEPTED, format!("is host"));
            } else {
                match session_lock.subscribe_to_host_candidate_response(&token) {
                    Some(future_result) => future_result,

                    None => return (StatusCode::FORBIDDEN, format!("not a host candidate")),
                }
            }
        };

        let result = future_result.await;
        if result.is_err() {
            (StatusCode::FORBIDDEN, format!("not a host candidate"))
        } else {
            match result.unwrap() {
                HostCandidateResponse::Accepted => (StatusCode::ACCEPTED, format!("is host")),
                HostCandidateResponse::Rejected => {
                    (StatusCode::FORBIDDEN, format!("not a host candidate"))
                }
                HostCandidateResponse::Overwritten => {
                    (StatusCode::BAD_REQUEST, format!("acceptance overwritten"))
                }
            }
        }
    }

    /// creates and runs a new input server
    /// function does not return until a host has connected
    pub fn run_new(address: String) -> Self {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("failed to install default rustls CryptoProvider");

        let socket_addr: SocketAddr = address.parse().expect("Unable to parse address");

        let certified_key =
            rcgen::generate_simple_self_signed(vec![format!("{}{}", address, Self::PLAYER_PATH)])
                .expect("Unable to generate https certificate.");

        let (player_input_sender, input_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();

        let state = Arc::new(Mutex::new(ServerState::new(
            player_input_sender,
            event_sender,
        )));

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
                Self::NEW_HOST_CANDIDATE_PATH,
                post(Self::new_host_candidate_handler),
            )
            .route(
                Self::HOST_ACCEPTANCE_PATH,
                post(Self::host_acceptance_handler),
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
            .route(
                Self::PLAYER_WEBSOCKET_PATH,
                axum::routing::any(Self::player_websocket_handler),
            )
            .route(Self::HOST_API_PATH, post(Self::host_api_handler))
            .with_state(state.clone().into());

        let handle = Handle::new();
        let handle_clone = handle.clone();
        let address_clone = address.clone();

        let should_start = Arc::new(AtomicBool::new(false));
        let should_start_clone = should_start.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime");

            rt.block_on(async move {
                let tls_config = RustlsConfig::from_pem(
                    certified_key.cert.pem().into_bytes(),
                    certified_key.signing_key.serialize_pem().into_bytes(),
                )
                .await
                .expect("Unable to create TLS configuration");

                should_start_clone.store(true, std::sync::atomic::Ordering::Relaxed);

                axum_server::bind_rustls(socket_addr, tls_config)
                    .handle(handle_clone)
                    .serve(app.into_make_service())
                    .await
                    .expect("Server failed to run");

                /*
                                let listener = tokio::net::TcpListener::bind(&address_clone).await.unwrap();
                                axum::serve(listener, app).await.unwrap();
                */
            });
        });

        loop {
            std::thread::sleep(Self::OPEN_SERVER_POLL_INTERVAL);
            if should_start.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Self::OPEN_SERVER_POLL_INTERVAL);
                break;
            }
        }
        println!("Server running on http://{}", &address_clone);
        println!("Be sure to open this device's hotspot and ensure everyone is connected.");
        println!(
            "Lobby will open once host enters the code at http://{address_clone}{}",
            Self::HOST_PATH
        );
        loop {
            print!("Code: ");
            std::io::stdout().flush().unwrap();

            let mut input = String::new();

            if std::io::stdin().read_line(&mut input).is_err() {
                println!("Invalid code!");
                continue;
            }

            match HostJoinCode::from_str(&input) {
                Ok(code) => {
                    if state.lock().unwrap().claim_host(code).is_ok() {
                        break;
                    } else {
                        println!("Invalid code!");
                        continue;
                    }
                }

                Err(_) => {
                    println!("Invalid code!");
                    continue;
                }
            }
        }
        println!("Host connected. Opening lobby...");

        let out = Self {
            ip_addr: address,
            state,
            input_receiver,
            event_receiver,
            server_handle: handle,
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
        self.state
            .lock()
            .expect("Unable to block new players")
            .block_joins();
    }

    /// allows new players to join
    /// (enabled upon server creation)
    pub fn allow_new_players(&self) {
        self.state
            .lock()
            .expect("Unable to allow new players")
            .allow_joins();
    }

    /// kicks the given player
    pub fn kick_player(&self, color: PlayerColor) {
        self.state
            .lock()
            .expect("Unable to kick player")
            .kick_player(color);
    }

    /// hint that player kicks are allowed or not
    pub fn set_kicks_allowed_hint(&self, allowed: bool) {
        self.state
            .lock()
            .expect("unable to update kicks allowed hint")
            .set_kicks_allowed_hint(allowed);
    }

    pub fn poll_inputs(&self) -> impl Iterator<Item = PlayerInputEvent> {
        self.input_receiver.try_iter()
    }

    pub fn poll_events(&self) -> impl Iterator<Item = ServerEvent> {
        self.event_receiver.try_iter()
    }

    pub fn claim_host(&self, code: HostJoinCode) -> Result<(), String> {
        self.state
            .lock()
            .expect("unable to claim host")
            .claim_host(code)
    }
}

impl Drop for GameServer {
    fn drop(&mut self) {
        self.server_handle
            .graceful_shutdown(Some(std::time::Duration::from_secs_f32(10.0)));
    }
}
