use crate::{core::models::ConditionDefinition, services::variables::EventContext, AppError};
use std::path::Path;

pub fn evaluate_condition(
    condition: &ConditionDefinition,
    context: &EventContext,
) -> Result<bool, AppError> {
    let path = Path::new(&context.file_path);
    Ok(match condition {
        ConditionDefinition::Filename { pattern } => wildcard_match(pattern, &context.file_name),
        ConditionDefinition::FolderName { pattern } => {
            wildcard_match(pattern, &context.folder_name)
        }
        ConditionDefinition::Extension { value } => context
            .extension
            .eq_ignore_ascii_case(value.trim_start_matches('.')),
        ConditionDefinition::Path { pattern } => wildcard_match(pattern, &context.file_path),
        ConditionDefinition::Exists { expected } => path.exists() == *expected,
    })
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut remaining = value;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(position) = remaining.find(part) else {
            return false;
        };
        if index == 0 && position != 0 {
            return false;
        }
        remaining = &remaining[position + part.len()..];
    }
    pattern.ends_with('*') || remaining.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_filename_pattern() {
        let context = EventContext {
            file_name: "report.csv".into(),
            ..Default::default()
        };
        assert!(evaluate_condition(
            &ConditionDefinition::Filename {
                pattern: "report.*".into()
            },
            &context
        )
        .unwrap());
    }
}
