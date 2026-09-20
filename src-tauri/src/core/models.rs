use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerDefinition {
    Manual,
    FolderCreated { path: String },
    FileCreated { path: String },
    FileModified { path: String },
    Interval { interval_seconds: u64, #[serde(default)] start_immediately: bool },
    Scheduled {
        schedule: String,
        #[serde(default)]
        timezone: Option<String>,
    },
}

impl TriggerDefinition {
    pub fn trigger_type(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::FolderCreated { .. } => "folder_created",
            Self::FileCreated { .. } => "file_created",
            Self::FileModified { .. } => "file_modified",
            Self::Interval { .. } => "interval",
            Self::Scheduled { .. } => "scheduled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConditionDefinition {
    Filename { pattern: String },
    FolderName { pattern: String },
    Extension { value: String },
    Path { pattern: String },
    Exists { expected: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionDefinition {
    Python {
        script: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Command {
        executable: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Shell {
        command: String,
    },
    Copy {
        source: String,
        destination: String,
    },
    Move {
        source: String,
        destination: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    Fixed,
    Exponential,
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Fixed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
    pub delay_seconds: u64,
    pub backoff: BackoffStrategy,
    pub max_delay_seconds: u64,
    pub retry_on_timeout: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_attempts: 1,
            delay_seconds: 0,
            backoff: BackoffStrategy::Fixed,
            max_delay_seconds: 300,
            retry_on_timeout: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlapPolicy {
    Skip,
    Queue,
}

impl Default for OverlapPolicy {
    fn default() -> Self {
        Self::Skip
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConcurrencyPolicy {
    pub max_workers: u32,
    pub overlap_policy: OverlapPolicy,
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        Self {
            max_workers: 1,
            overlap_policy: OverlapPolicy::Skip,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeduplicationPolicy {
    pub enabled: bool,
    pub window_seconds: u64,
}

impl Default for DeduplicationPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            window_seconds: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPolicy {
    pub timeout_seconds: u64,
    pub retry: RetryPolicy,
    pub concurrency: ConcurrencyPolicy,
    pub deduplication: DeduplicationPolicy,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            timeout_seconds: 300,
            retry: RetryPolicy::default(),
            concurrency: ConcurrencyPolicy::default(),
            deduplication: DeduplicationPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger: TriggerDefinition,
    pub conditions: Vec<ConditionDefinition>,
    pub actions: Vec<ActionDefinition>,
    pub execution: ExecutionPolicy,
}

impl AutomationDefinition {
    pub fn minimal(
        name: impl Into<String>,
        trigger: TriggerDefinition,
        execution: ExecutionPolicy,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            description: String::new(),
            enabled: true,
            trigger,
            conditions: Vec::new(),
            actions: vec![ActionDefinition::Command {
                executable: "true".to_string(),
                args: Vec::new(),
            }],
            execution,
        }
    }
}
