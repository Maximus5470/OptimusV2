use crate::types::{Language, JobRequest};
use redis::{AsyncCommands, RedisResult};

/// Redis queue semantics - defines only semantics, not runtime logic
/// Ensures API and worker never drift, Redis keys are deterministic,
/// and KEDA scaling remains predictable

pub const QUEUE_PREFIX: &str = "optimus:queue";
pub const RESULT_PREFIX: &str = "optimus:result";
pub const STATUS_PREFIX: &str = "optimus:status";
pub const METRICS_PREFIX: &str = "optimus:metrics";

/// Generate deterministic queue name for a language
pub fn queue_name(language: &Language) -> String {
    format!("{}:{}", QUEUE_PREFIX, language)
}

/// Generate retry queue name for a language
pub fn retry_queue_name(language: &Language) -> String {
    format!("{}:{}:retry", QUEUE_PREFIX, language)
}

/// Generate dead letter queue name for a language
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

/// Push a job to the language-specific queue
/// Uses RPUSH for FIFO semantics
pub async fn push_job(
    conn: &mut redis::aio::ConnectionManager,
    job: &JobRequest,
) -> RedisResult<()> {
    let queue = queue_name(&job.language);
    let payload = serde_json::to_string(job)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    conn.rpush(&queue, payload).await
}

/// Push a job to the retry queue
pub async fn push_to_retry_queue(
    conn: &mut redis::aio::ConnectionManager,
    job: &JobRequest,
) -> RedisResult<()> {
    let queue = retry_queue_name(&job.language);
    let payload = serde_json::to_string(job)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    conn.rpush(&queue, payload).await
}

/// Push a job to the dead letter queue
pub async fn push_to_dlq(
    conn: &mut redis::aio::ConnectionManager,
    job: &JobRequest,
) -> RedisResult<()> {
    let queue = dlq_name(&job.language);
    let payload = serde_json::to_string(job)
        .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "serialization error", e.to_string())))?;
    
    conn.rpush(&queue, payload).await
}

/// Pop a job from the language-specific queue
/// Uses BLPOP with timeout for graceful shutdown
pub async fn pop_job(
    conn: &mut redis::aio::ConnectionManager,
    language: &Language,
    timeout_seconds: f64,
) -> RedisResult<Option<JobRequest>> {
    let queue = queue_name(language);
    let result: Option<(String, String)> = conn.blpop(&queue, timeout_seconds).await?;
    
    match result {
        Some((_key, payload)) => {
            let job: JobRequest = serde_json::from_str(&payload)
                .map_err(|e| redis::RedisError::from((redis::ErrorKind::TypeError, "deserialization error", e.to_string())))?;
            Ok(Some(job))
        }
        None => Ok(None),
    }
}

/// Pop a job from either the main queue or retry queue (priority: main first)
/// Uses BLPOP with multiple keys - Redis pops from first non-empty queue
pub async fn pop_job_with_retry(
    conn: &mut redis::aio::ConnectionManager,
    language: &Language,
    timeout_seconds: f64,
) -> RedisResult<Option<JobRequest>> {
    let main_queue = queue_name(language);
    let retry_queue = retry_queue_name(language);
    
    // BLPOP checks keys in order - main queue has priority
    let result: Option<(String, String)> = conn.blpop(&[main_queue, retry_queue], timeout_seconds).await?;
    
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Language;
    use uuid::Uuid;

    #[test]
    fn test_queue_naming() {
        assert_eq!(queue_name(&Language::Python), "optimus:queue:python");
        assert_eq!(queue_name(&Language::Java), "optimus:queue:java");
        assert_eq!(queue_name(&Language::Rust), "optimus:queue:rust");
        
        assert_eq!(retry_queue_name(&Language::Python), "optimus:queue:python:retry");
        assert_eq!(retry_queue_name(&Language::Java), "optimus:queue:java:retry");
        
        assert_eq!(dlq_name(&Language::Python), "optimus:queue:python:dlq");
        assert_eq!(dlq_name(&Language::Rust), "optimus:queue:rust:dlq");
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
