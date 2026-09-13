use std::path::PathBuf;

use gatheround::{app::run_app_with_settings, raylib_integrations::RaylibPlatform};
use raylib::ffi::Vector2;

#[tokio::main]
async fn main() {
    let dimensions = Vector2 {
        x: 1000.0,
        y: 800.0,
    };

    let platform = RaylibPlatform::new(dimensions);

    run_app_with_settings(
        &PathBuf::from("C:/Users/sfman/cs-projects/gatheround/lua_example"),
        "127.0.0.1:443",
        dimensions,
        platform,
    ).await;
}
