use std::path::PathBuf;

use gatheround::{
    app::{AppState, GameSettings},
    platform::{IPlatform, raylib},
    utils::geometry::IVec2,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    type V = <raylib::Platform as IPlatform>::Vector2;

    let settings = GameSettings {
        games_directory: PathBuf::from(r"C:\Users\sfman\OneDrive\Desktop\tempgames"),
        ip_address: String::from("192.168.137.1:3000"),
        dimensions: V::from_xy(1920.0, 1200.0),
    };

    let platform_generator =
        |settings: &GameSettings<V>| raylib::Platform::new(settings.dimensions);

    let mut app = AppState::new_with_settings(settings, platform_generator).await;

    app.run();
}
