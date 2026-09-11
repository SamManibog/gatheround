use std::sync::mpsc::{self, Receiver};

use axum::{Json, Router, http::StatusCode, routing::post};
use tokio::{self, task::JoinHandle};
use tower_http::services::ServeDir;

use crate::input::{DeserializableInputEvent, InputEvent, PlayerInputEvent};

pub struct GameServer {
    input_receiver: Receiver<PlayerInputEvent>,
    service_thread: JoinHandle<Result<(), tokio::io::Error>>,
}

impl GameServer {
    pub const IP_PORT: &str = "192.168.137.1:443";

    /// creates and runs a new input server
    pub async fn run_new() -> Self {
        let port = Self::IP_PORT;

        let (sender, receiver) = mpsc::channel();

        let input_handler = async move |Json(playload): Json<DeserializableInputEvent>| {
            match <InputEvent as TryFrom<&DeserializableInputEvent>>::try_from(&playload) {
                Err(msg) => (StatusCode::BAD_REQUEST, msg),
                Ok(event) => {
                    let player_event = PlayerInputEvent {
                        player: crate::resources::PlayerColor::Lime,
                        event,
                    };
                    let _ = sender
                        .send(player_event)
                        .expect("Could not send input to main thread.");
                    (StatusCode::ACCEPTED, "Input received.".to_owned())
                }
            }
        };

        let app = Router::new()
            .route("/input", post(input_handler))
            .fallback_service(ServeDir::new("./static"));

        let listener = tokio::net::TcpListener::bind(Self::IP_PORT)
            .await
            .expect(&format!("Failed to bind to port: {port}."));
        let service_thread = tokio::spawn(async move { axum::serve(listener, app).await });

        Self {
            input_receiver: receiver,
            service_thread,
        }
    }

    pub fn poll_inputs(&self) -> impl Iterator<Item = PlayerInputEvent> {
        self.input_receiver.try_iter()
    }

    pub fn abort(self) {
        self.service_thread.abort();
    }
}
