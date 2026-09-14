use std::path::PathBuf;

use gatheround::{app::AppState, raylib_integrations::RaylibPlatform};
use raylib::ffi::Vector2;

#[tokio::main]
async fn main() {
    let dimensions = Vector2 {
        x: 1920.0,
        y: 1200.0,
    };

    let platform = RaylibPlatform::new(dimensions);

    let mut app = AppState::new_with_settings(
        PathBuf::from("C:/Users/sfman/cs-projects/gatheround/lua_example"),
        String::from("127.0.0.1:3000"),
        dimensions,
        platform,
    ).await;

    app.run();
}
