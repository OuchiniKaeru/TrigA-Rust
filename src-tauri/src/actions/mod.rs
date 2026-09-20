pub mod process;

use crate::{
    core::models::ActionDefinition,
    services::{
        security::SecurityPolicy,
        variables::{expand_variables, EventContext},
    },
    AppError,
};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub async fn execute(
    action: &ActionDefinition,
    context: &EventContext,
    limit: Duration,
    security: &SecurityPolicy,
) -> Result<ActionResult, AppError> {
    match action {
        ActionDefinition::Python { script, args } => {
            let executable = if cfg!(windows) { "python" } else { "python3" };
            security.validate_executable(executable)?;
            run_process(
                executable,
                std::iter::once(script.as_str()).chain(args.iter().map(String::as_str)),
                context,
                limit,
            )
            .await
        }
        ActionDefinition::Command { executable, args } => {
            security.validate_executable(executable)?;
            run_process(executable, args.iter().map(String::as_str), context, limit).await
        }
        ActionDefinition::Shell { command } => {
            let expanded = expand_variables(command, context)?;
            if expanded.trim().is_empty() {
                return Err(AppError::Validation("shell command must not be empty".into()));
            }
            // Shell action is intentionally explicit: a fixed interpreter is
            // invoked with the expanded script as a single `-c` argument.
            // The interpreter itself must pass the allowlist when configured.
            if cfg!(windows) {
                security.validate_executable("cmd")?;
                run_process(
                    "cmd",
                    ["/C", expanded.as_str()],
                    context,
                    limit,
                )
                .await
            } else {
                security.validate_executable("sh")?;
                run_process("sh", ["-c", expanded.as_str()], context, limit).await
            }
        }
        ActionDefinition::Copy {
            source,
            destination,
        } => {
            let source = expand_variables(source, context)?;
            let destination = expand_variables(destination, context)?;
            let source_path = security.validate_path(std::path::Path::new(&source))?;
            let destination_path = security.validate_path(std::path::Path::new(&destination))?;
            if let Some(parent) = destination_path.parent() {
                if !parent.as_os_str().is_empty() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|error| AppError::Action(error.to_string()))?;
                }
            }
            tokio::fs::copy(&source_path, &destination_path)
                .await
                .map_err(|error| AppError::Action(error.to_string()))?;
            Ok(ActionResult {
                status: "success".into(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
        ActionDefinition::Move {
            source,
            destination,
        } => {
            let source = expand_variables(source, context)?;
            let destination = expand_variables(destination, context)?;
            let source_path = security.validate_path(std::path::Path::new(&source))?;
            let destination_path = security.validate_path(std::path::Path::new(&destination))?;
            if let Some(parent) = destination_path.parent() {
                if !parent.as_os_str().is_empty() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|error| AppError::Action(error.to_string()))?;
                }
            }
            tokio::fs::rename(&source_path, &destination_path)
                .await
                .map_err(|error| AppError::Action(error.to_string()))?;
            Ok(ActionResult {
                status: "success".into(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

async fn run_process<'a, I>(
    executable: &str,
    args: I,
    context: &EventContext,
    limit: Duration,
) -> Result<ActionResult, AppError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut command = Command::new(executable);
    for arg in args {
        command.arg(expand_variables(arg, context)?);
    }
    let child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| AppError::Process(error.to_string()))?;
    // `kill_on_drop(true)` guarantees the child is terminated if the
    // timeout future is dropped, so no orphaned subprocess remains
    // (spec #32.1: do not orphan child processes).
    match timeout(limit, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(ActionResult {
            status: if output.status.success() {
                "success"
            } else {
                "failed"
            }
            .into(),
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        }),
        Ok(Err(error)) => Err(AppError::Process(error.to_string())),
        Err(_) => Err(AppError::Timeout),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::ActionDefinition;
    #[tokio::test]
    async fn runs_command_without_shell_interpolation() {
        let executable = if cfg!(windows) { "cmd" } else { "printf" };
        let args = if cfg!(windows) {
            vec!["/C", "echo", "ok"]
        } else {
            vec!["ok"]
        };
        let action = ActionDefinition::Command {
            executable: executable.into(),
            args: args.into_iter().map(String::from).collect(),
        };
        let result = execute(
            &action,
            &EventContext::default(),
            Duration::from_secs(2),
            &SecurityPolicy::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, "success");
    }
}
