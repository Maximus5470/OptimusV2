// Prometheus metrics for Optimus API

use lazy_static::lazy_static;
use prometheus::{
    CounterVec, HistogramOpts, HistogramVec, IntGaugeVec, Opts,
    Registry, TextEncoder, Encoder,
};

lazy_static! {
    // Global registry
    pub static ref REGISTRY: Registry = Registry::new();

    // Jobs submitted total (counter with language label)
    pub static ref JOBS_SUBMITTED: CounterVec = CounterVec::new(
        Opts::new("optimus_jobs_submitted_total", "Total number of jobs submitted"),
        &["language"]
    )
    .expect("metric can be created");

    // Jobs completed total (counter with language and status labels)
    pub static ref JOBS_COMPLETED: CounterVec = CounterVec::new(
        Opts::new("optimus_jobs_completed_total", "Total number of jobs completed"),
        &["language", "status"]
    )
    .expect("metric can be created");

    // Job execution time histogram (in milliseconds)
    pub static ref JOB_EXECUTION_TIME: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "optimus_job_execution_time_ms",
            "Job execution time in milliseconds"
        )
        .buckets(vec![100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 30000.0]),
        &["language"]
    )
    .expect("metric can be created");

    // Queue depth gauge (current depth per language)
    pub static ref QUEUE_DEPTH: IntGaugeVec = IntGaugeVec::new(
        Opts::new("optimus_queue_depth", "Current queue depth per language"),
        &["language"]
    )
    .expect("metric can be created");

    // API request counter
    pub static ref API_REQUESTS: CounterVec = CounterVec::new(
        Opts::new("optimus_api_requests_total", "Total API requests"),
        &["endpoint", "method", "status"]
    )
    .expect("metric can be created");

    // Jobs rejected counter (backpressure)
    pub static ref JOBS_REJECTED: CounterVec = CounterVec::new(
        Opts::new("optimus_jobs_rejected_total", "Total jobs rejected due to validation"),
        &["reason"]
    )
    .expect("metric can be created");
}

/// Initialize metrics registry
pub fn init_metrics() {
    REGISTRY
        .register(Box::new(JOBS_SUBMITTED.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(JOBS_COMPLETED.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(JOB_EXECUTION_TIME.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(QUEUE_DEPTH.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(API_REQUESTS.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(JOBS_REJECTED.clone()))
        .expect("collector can be registered");
}

/// Render metrics in Prometheus text format
pub fn render_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

/// Record job submission
pub fn record_job_submitted(language: &str) {
    JOBS_SUBMITTED.with_label_values(&[language]).inc();
}

/// Record job rejection
pub fn record_job_rejected(reason: &str) {
    JOBS_REJECTED.with_label_values(&[reason]).inc();
}

/// Record job completion
pub fn record_job_completed(language: &str, status: &str, execution_time_ms: f64) {
    JOBS_COMPLETED.with_label_values(&[language, status]).inc();
    JOB_EXECUTION_TIME.with_label_values(&[language]).observe(execution_time_ms);
}

/// Update queue depth for a language
pub async fn update_queue_depths(redis_conn: &mut redis::aio::ConnectionManager) {
    use redis::AsyncCommands;
    use optimus_common::types::Language;
    
    for language in Language::all_variants() {
        let queue_name = optimus_common::redis::queue_name(language);
        if let Ok(depth) = redis_conn.llen::<_, i64>(&queue_name).await {
            QUEUE_DEPTH
                .with_label_values(&[&language.to_string()])
                .set(depth);
        }
    }
}
