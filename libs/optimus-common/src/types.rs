use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Strongly-typed language enum
/// Start strict - will extend dynamically later
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Java,
    Rust,
}

/// Job Metadata for Retry and Failure Handling
/// Tracks retry attempts and failure information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetadata {
    pub attempts: u8,
    pub max_attempts: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_reason: Option<String>,
}

impl Default for JobMetadata {
    fn default() -> Self {
        Self {
            attempts: 0,
            max_attempts: 3,
            last_failure_reason: None,
        }
    }
}

/// Test Case Definition (Immutable Input)
/// Test cases are immutable - workers must not mutate them
/// Ordering matters - execution is sequential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: u32,
    pub input: String,
    pub expected_output: String,
    pub weight: u32, // for scoring
}

impl Language {
    /// Returns all language variants
    /// This is the single source of truth for available languages
    /// Add new languages here and they'll automatically propagate everywhere
    pub fn all_variants() -> &'static [Language] {
        &[Language::Python, Language::Java, Language::Rust]
    }
    
    /// Parse a language from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Language> {
        match s.to_lowercase().as_str() {
            "python" => Some(Language::Python),
            "java" => Some(Language::Java),
            "rust" => Some(Language::Rust),
            _ => None,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Language::Python => write!(f, "python"),
            Language::Java => write!(f, "java"),
            Language::Rust => write!(f, "rust"),
        }
    }
}

/// Job Input (Immutable)
/// A job is write-once - never mutate input fields
/// 
/// ## Test Case Execution Semantics:
/// - Test cases execute **sequentially** in order
/// - First runtime crash may stop execution (configurable later)
/// - Timeout applies per test case
/// - Test cases are mandatory (empty vec = instant completion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub id: Uuid,
    pub language: Language,
    pub source_code: String,
    pub test_cases: Vec<TestCase>,
    pub timeout_ms: u64,
    #[serde(default)]
    pub metadata: JobMetadata,
}

/// Job State Machine
/// Explicitly models lifecycle states
/// Backs: GET /job/{id}, retry logic, metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    TimedOut,
}

/// Per-Test Status
/// Distinguishes different failure modes for individual test cases
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
    RuntimeError,
    TimeLimitExceeded,
}

/// Per-Test Result
/// Captures individual test case execution outcome
/// Enables partial success and detailed feedback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: u32,
    pub status: TestStatus,
    pub stdout: String,
    pub stderr: String,
    pub execution_time_ms: u64,
}

/// Execution Output
/// Written by workers, read by API, stored in Redis/object storage
/// 
/// ## Scoring Semantics:
/// - score: sum of weights for passed tests
/// - max_score: sum of all test case weights
/// - overall_status: Completed if all tests passed, Failed otherwise
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub job_id: Uuid,
    pub overall_status: JobStatus,
    pub score: u32,
    pub max_score: u32,
    pub results: Vec<TestResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_serialization() {
        let lang = Language::Python;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"python\"");
        
        let deserialized: Language = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Language::Python);
    }

    #[test]
    fn test_job_request_serialization() {
        let test_cases = vec![
            TestCase {
                id: 1,
                input: "5\n".to_string(),
                expected_output: "120\n".to_string(),
                weight: 10,
            },
            TestCase {
                id: 2,
                input: "3\n".to_string(),
                expected_output: "6\n".to_string(),
                weight: 10,
            },
        ];
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: "public class Main {}".to_string(),
            test_cases,
            timeout_ms: 5000,
            metadata: JobMetadata::default(),
        };
        
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: JobRequest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.language, Language::Java);
        assert_eq!(deserialized.timeout_ms, 5000);
        assert_eq!(deserialized.test_cases.len(), 2);
        assert_eq!(deserialized.test_cases[0].weight, 10);
    }

    #[test]
    fn test_job_status_serialization() {
        let status = JobStatus::Completed;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"completed\"");
    }

    #[test]
    fn test_execution_result_structure() {
        let test_results = vec![
            TestResult {
                test_id: 1,
                status: TestStatus::Passed,
                stdout: "120\n".to_string(),
                stderr: String::new(),
                execution_time_ms: 45,
            },
            TestResult {
                test_id: 2,
                status: TestStatus::Failed,
                stdout: "5\n".to_string(),
                stderr: String::new(),
                execution_time_ms: 42,
            },
        ];
        
        let result = ExecutionResult {
            job_id: Uuid::new_v4(),
            overall_status: JobStatus::Completed,
            score: 10,
            max_score: 20,
            results: test_results,
        };
        
        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 10);
        assert_eq!(result.max_score, 20);
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::Failed);
    }
    
    #[test]
    fn test_test_case_immutability() {
        let test_case = TestCase {
            id: 1,
            input: "input".to_string(),
            expected_output: "output".to_string(),
            weight: 5,
        };
        
        // Test case can be cloned but original is immutable
        let cloned = test_case.clone();
        assert_eq!(cloned.id, test_case.id);
        assert_eq!(cloned.weight, 5);
    }
    
    #[test]
    fn test_test_status_serialization() {
        let status = TestStatus::Passed;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"passed\"");
        
        let failed = TestStatus::Failed;
        let json = serde_json::to_string(&failed).unwrap();
        assert_eq!(json, "\"failed\"");
    }
    
    #[test]
    fn test_language_all_variants() {
        let variants = Language::all_variants();
        assert_eq!(variants.len(), 3);
        assert!(variants.contains(&Language::Python));
        assert!(variants.contains(&Language::Java));
        assert!(variants.contains(&Language::Rust));
    }
    
    #[test]
    fn test_language_from_str() {
        assert_eq!(Language::from_str("python"), Some(Language::Python));
        assert_eq!(Language::from_str("Python"), Some(Language::Python));
        assert_eq!(Language::from_str("PYTHON"), Some(Language::Python));
        
        assert_eq!(Language::from_str("java"), Some(Language::Java));
        assert_eq!(Language::from_str("rust"), Some(Language::Rust));
        
        assert_eq!(Language::from_str("javascript"), None);
        assert_eq!(Language::from_str(""), None);
    }
}
