pub mod app;
pub mod assets;
pub mod board;
pub mod cheat_buffer;
pub mod game;
pub mod input;
pub mod lua;
pub mod platform;
pub mod server;
pub mod utils;

#[allow(unused)]
pub(crate) mod profiling {
    use std::time::Instant;

    pub struct Timer {
        name: &'static str,
        start: Instant,
    }

    impl Timer {
        pub fn start(name: &'static str) -> Self {
            Self {
                name,
                start: Instant::now()
            }
        }

        pub fn end(self) {
            let end = Instant::now();
            let duration = end - self.start;
            println!("Time for {}: {} ms", self.name, duration.as_secs_f64() / 1000.0)
        }
    }
}
