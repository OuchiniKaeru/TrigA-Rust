use crate::AppError;

#[derive(Debug, Clone)]
pub struct WorkflowError {
    pub message: String,
    pub retryable: bool,
}

impl WorkflowError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }
    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<WorkflowError> for AppError {
    fn from(value: WorkflowError) -> Self {
        if value.retryable {
            AppError::Workflow(value.message)
        } else {
            AppError::Validation(value.message)
        }
    }
}

/// Non-retryable error classes (spec 7.3): Cancelled, Validation, Security,
/// Invalid Configuration.
pub fn is_retryable_message(message: &str) -> bool {
    !(message.contains("validation error")
        || message.contains("security error")
        || message.contains("cancelled")
        || message.contains("invalid configuration"))
}
