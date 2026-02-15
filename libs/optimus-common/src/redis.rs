use crate::types::{Language, JobRequest};
use redis::{AsyncCommands, RedisResult};

/// Redis queue semantics - defines only semantics, not runtime logic
/// Ensures API and worker never drift, Redis keys are deterministic,
/// and KEDA scaling remains predictable

pub const QUEUE_PREFIX: &str = "optimus:queue";
pub const RESULT_PREFIX: &str = "optimus:result";
pub const STATUS_PREFIX: &str = "optimus:status";
pub const METRICS_PREFIX: &str = "optimus:metrics";
pub const CONTROL_PREFIX: &str = "optimus:control";

// ===== UNIFIED QUEUE (Universal Worker Architecture) =====
// All languages share a single queue. Workers pick up any job and
// select the correct sandbox image at execution time.
pub const UNIFIED_QUEUE: &str = "optimus:queue:jobs";
pub const UNIFIED_RETRY_QUEUE: &str = "optimus:queue:jobs:retry";
pub const UNIFIED_DLQ: &str = "optimus:queue:jobs:dlq";

/// Generate deterministic queue name for a language
#[deprecated(note = "Use UNIFIED_QUEUE constant instead — Universal Worker architecture")]
pub fn queue_name(language: &Language) -> String {
    format!("{}:{}", QUEUE_PREFIX, language)
}

/// Generate retry queue name for a language
#[deprecated(note = "Use UNIFIED_RETRY_QUEUE constant instead — Universal Worker architecture")]
pub fn retry_queue_name(language: &Language) -> String {
    format!("{}:{}:retry", QUEUE_PREFIX, language)
}

/// Generate dead letter queue name for a language
#[deprecated(note = "Use UNIFIED_DLQ constant instead — Universal Worker architecture")]
pub fn dlq_name(language: &Language) -> String {
    format!("{}:{}:dlq", QUEUE_PREFIX, language)
}

/// Generate result key for a job
pub fn result_key(job_id: &uuid::Uuid) -> String {
    format!("{}:{}", RESULT_PREFIX, job_id)
}

/// Generate status key for a job
pub fn status_key(job_id: &uuid::Uuid) -> String {
    format!("{}:{}", STATUS_PREFIX, job_id)
}

/// Generate control key for a job (cancellation flag)
pub fn control_key(job_id: &uuid::Uuid) -> String {
    format!("{}:{}", CONTROL_PREFIX, job_id)
}

/// Push a job to the unified queue (all languages)
/// Uses RPUSH for FIFO semantics
pub async fn push_job(
    conn: &mut redis::aio::ConnectionManager,
    job: &JobRequest,
) -> RedisResult<()> {
    let payload = serde_json::to_string(job)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    conn.rpush(UNIFIED_QUEUE, payload).await
}

/// Push a job to the unified retry queue
pub async fn push_to_retry_queue(
    conn: &mut redis::aio::ConnectionManager,
    job: &JobRequest,
) -> RedisResult<()> {
    let payload = serde_json::to_string(job)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    conn.rpush(UNIFIED_RETRY_QUEUE, payload).await
}

/// Push a job to the unified dead letter queue
pub async fn push_to_dlq(
    conn: &mut redis::aio::ConnectionManager,
    job: &JobRequest,
) -> RedisResult<()> {
    let payload = serde_json::to_string(job)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    conn.rpush(UNIFIED_DLQ, payload).await
}

/// Pop a job from the unified queue (language-agnostic)
/// Uses BLPOP with timeout for graceful shutdown
/// Workers use this to pick up ANY language job
pub async fn pop_job(
    conn: &mut redis::aio::ConnectionManager,
    timeout_seconds: f64,
) -> RedisResult<Option<JobRequest>> {
    let result: Option<(String, String)> = conn.blpop(UNIFIED_QUEUE, timeout_seconds).await?;
    
    match result {
        Some((_key, payload)) => {
            let job: JobRequest = serde_json::from_str(&payload)
                .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "deserialization error", e.to_string())))?;
            Ok(Some(job))
        }
        None => Ok(None),
    }
}

/// Pop a job from unified main queue or retry queue (priority: main first)
/// Uses BLPOP with multiple keys - Redis pops from first non-empty queue
/// Language-agnostic: any worker can pick up any job
pub async fn pop_job_with_retry(
    conn: &mut redis::aio::ConnectionManager,
    timeout_seconds: f64,
) -> RedisResult<Option<JobRequest>> {
    // BLPOP checks keys in order - main queue has priority over retry
    let result: Option<(String, String)> = conn.blpop(&[UNIFIED_QUEUE, UNIFIED_RETRY_QUEUE], timeout_seconds).await?;
    
    match result {
        Some((_key, payload)) => {
            let job: JobRequest = serde_json::from_str(&payload)
                .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "deserialization error", e.to_string())))?;
            Ok(Some(job))
        }
        None => Ok(None),
    }
}

/// Store execution result in Redis
/// TTL is optional - set to 24 hours for now (can be configured later)
/// 
/// Also publishes metrics event for distributed tracking
pub async fn store_result(
    conn: &mut redis::aio::ConnectionManager,
    result: &crate::types::ExecutionResult,
) -> RedisResult<()> {
    let key = result_key(&result.job_id);
    let payload = serde_json::to_string(result)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    // Store result with 24-hour TTL
    let _: () = conn.set_ex(&key, payload, 86400).await?;
    
    // Also store status separately for quick lookup
    let status_key_str = status_key(&result.job_id);
    let status_str = serde_json::to_string(&result.overall_status)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    let _: () = conn.set_ex(&status_key_str, status_str, 86400).await?;
    
    Ok(())
}

/// Store execution result and publish completion metrics
/// This is a convenience function that combines store_result with metrics publishing
pub async fn store_result_with_metrics(
    conn: &mut redis::aio::ConnectionManager,
    result: &crate::types::ExecutionResult,
    language: &crate::types::Language,
) -> RedisResult<()> {
    // Store the result first
    store_result(conn, result).await?;
    
    // Publish metrics event
    publish_job_completion(conn, result, language).await?;
    
    Ok(())
}

/// Publish job completion metrics (for distributed metrics tracking)
async fn publish_job_completion(
    conn: &mut redis::aio::ConnectionManager,
    result: &crate::types::ExecutionResult,
    language: &crate::types::Language,
) -> RedisResult<()> {
    // Calculate total execution time from test results
    let total_execution_time_ms: u64 = result.results.iter()
        .map(|r| r.execution_time_ms)
        .sum();
    
    let channel = format!("{}:completions", METRICS_PREFIX);
    let event = serde_json::json!({
        "job_id": result.job_id.to_string(),
        "language": language.to_string(),
        "status": format!("{:?}", result.overall_status),
        "execution_time_ms": total_execution_time_ms,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    
    let payload = serde_json::to_string(&event)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    // Publish event (fire-and-forget, no subscribers required)
    let _: i64 = conn.publish(&channel, payload).await.unwrap_or(0);
    Ok(())
}

/// Retrieve execution result from Redis
pub async fn get_result(
    conn: &mut redis::aio::ConnectionManager,
    job_id: &uuid::Uuid,
) -> RedisResult<Option<crate::types::ExecutionResult>> {
    let key = result_key(job_id);
    let payload: Option<String> = conn.get(&key).await?;
    
    match payload {
        Some(data) => {
            let result: crate::types::ExecutionResult = serde_json::from_str(&data)
                .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "deserialization error", e.to_string())))?;
            Ok(Some(result))
        }
        None => Ok(None),
    }
}

/// Set cancellation flag for a job
/// TTL of 24 hours to match result expiry
pub async fn set_job_cancelled(
    conn: &mut redis::aio::ConnectionManager,
    job_id: &uuid::Uuid,
) -> RedisResult<()> {
    let key = control_key(job_id);
    let control = crate::types::JobControl { cancelled: true };
    let payload = serde_json::to_string(&control)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    // Store with 24-hour TTL
    conn.set_ex(&key, payload, 86400).await
}

/// Check if a job has been cancelled
pub async fn is_job_cancelled(
    conn: &mut redis::aio::ConnectionManager,
    job_id: &uuid::Uuid,
) -> RedisResult<bool> {
    let key = control_key(job_id);
    let payload: Option<String> = conn.get(&key).await?;
    
    match payload {
        Some(data) => {
            let control: crate::types::JobControl = serde_json::from_str(&data)
                .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "deserialization error", e.to_string())))?;
            Ok(control.cancelled)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_unified_queue_constants() {
        assert_eq!(UNIFIED_QUEUE, "optimus:queue:jobs");
        assert_eq!(UNIFIED_RETRY_QUEUE, "optimus:queue:jobs:retry");
        assert_eq!(UNIFIED_DLQ, "optimus:queue:jobs:dlq");
    }

    #[test]
    fn test_result_key_deterministic() {
        let id = Uuid::new_v4();
        let key1 = result_key(&id);
        let key2 = result_key(&id);
        assert_eq!(key1, key2);
        assert!(key1.starts_with("optimus:result:"));
    }

    #[test]
    fn test_status_key_format() {
        let id = Uuid::new_v4();
        let key = status_key(&id);
        assert!(key.starts_with("optimus:status:"));
        assert!(key.contains(&id.to_string()));
    }
}
