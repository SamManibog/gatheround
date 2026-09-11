#[tokio::main]
async fn main() {
    gatheround::raylib_integrations::run_game(
        &std::env::current_dir()
            .expect("Current directory does not exist.")
            .join("lua_example"),
    ).await;
}
