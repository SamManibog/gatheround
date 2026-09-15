use std::path::PathBuf;

use gatheround::{
    app::{AppState, GameSettings},
    raylib_integrations::RaylibPlatform,
};
use raylib::ffi::Vector2;

#[tokio::main]
async fn main() {
    let dimensions = Vector2 {
        x: 1920.0,
        y: 1200.0,
    };

    let settings = GameSettings {
        games_directory: PathBuf::from(r"C:\Users\sfman\OneDrive\Desktop\tempgames"),
        ip_address: String::from("127.0.0.1:3000"),
        dimensions,
    };

    let platform_generator =
        |settings: &GameSettings<Vector2>| RaylibPlatform::new(settings.dimensions);

    let mut app = AppState::new_with_settings(settings, platform_generator).await;

    app.run();
}
