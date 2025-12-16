// Application configuration
// Placeholder for now

pub struct Config {
    pub redis_host: String,
    pub timeout_seconds: u64,
}

impl Config {
    pub fn new() -> Self {
        Self {
            redis_host: "localhost:6379".to_string(),
            timeout_seconds: 30,
        }
    }
}
