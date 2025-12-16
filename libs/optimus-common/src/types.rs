use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub id: Uuid,
    pub language: String,
    pub source_code: String,
    pub test_cases: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub input: String,
    pub expected_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub job_id: Uuid,
    pub status: ExecutionStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub test_results: Vec<TestResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_case: TestCase,
    pub passed: bool,
    pub actual_output: String,
}
