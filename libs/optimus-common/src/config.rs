use std::env;

/// Application configuration
/// Provides defaults with environment variable overrides
#[derive(Debug, Clone)]
pub struct Config {
    pub redis_url: String,
    pub default_timeout_ms: u64,
    pub max_timeout_ms: u64,
}

/// Worker concurrency configuration
/// Controls parallelism to prevent resource oversubscription
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Maximum jobs executing in parallel on this worker
    /// Default: 3 (balanced throughput - adjust based on resources)
    /// PERFORMANCE: Increased from 1 to 3 for better queue processing
    /// Set MAX_PARALLEL_JOBS env var to tune per deployment
    pub max_parallel_jobs: usize,
    
    /// Maximum test cases executing in parallel within a single job
    /// Default: 1 (strict isolation - sequential execution within job)
    pub max_parallel_tests: usize,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            default_timeout_ms: env::var("DEFAULT_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
            max_timeout_ms: env::var("MAX_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30000),
        }
    }

    pub fn new() -> Self {
        Self::from_env()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerConfig {
    pub fn from_env() -> Self {
        Self {
            max_parallel_jobs: env::var("OPTIMUS_MAX_PARALLEL_JOBS")
                .or_else(|_| env::var("MAX_PARALLEL_JOBS"))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),  // PERFORMANCE FIX: Changed from 1 to 3
            max_parallel_tests: env::var("OPTIMUS_MAX_PARALLEL_TESTS")
                .or_else(|_| env::var("MAX_PARALLEL_TESTS"))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
        }
    }
    
    pub fn new() -> Self {
        Self::from_env()
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.default_timeout_ms, 5000);
        assert_eq!(config.max_timeout_ms, 30000);
    }
    
    #[test]
    fn test_worker_config_defaults() {
        let config = WorkerConfig::default();
        assert_eq!(config.max_parallel_jobs, 3);  // Updated from 1 to 3
        assert_eq!(config.max_parallel_tests, 1);
    }
}
