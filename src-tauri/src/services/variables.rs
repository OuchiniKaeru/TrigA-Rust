use crate::AppError;

#[derive(Debug, Clone, Default)]
pub struct EventContext {
    pub file_path: String,
    pub file_name: String,
    pub file_name_without_ext: String,
    pub folder_path: String,
    pub folder_name: String,
    pub extension: String,
    pub timestamp: String,
    pub date: String,
    pub time: String,
    pub automation_id: String,
    pub job_id: String,
    pub attempt: u32,
}

impl EventContext {
    pub fn for_trigger(
        automation_id: &str,
        job_id: &str,
        trigger_path: Option<&str>,
    ) -> Self {
        let now = chrono::Utc::now();
        let (file_path, folder_path) = match trigger_path {
            Some(p) => {
                let path = std::path::Path::new(p);
                if p.ends_with('/') || p.ends_with('\\') || path.is_dir() {
                    (String::new(), p.to_string())
                } else {
                    (
                        p.to_string(),
                        path.parent()
                            .map(|v| v.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    )
                }
            }
            None => (String::new(), String::new()),
        };
        let file_name = std::path::Path::new(&file_path)
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = std::path::Path::new(&file_path)
            .extension()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_name_without_ext = file_name
            .strip_suffix(&format!(".{extension}"))
            .unwrap_or(&file_name)
            .to_string();
        let folder_name = std::path::Path::new(&folder_path)
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            file_path,
            file_name,
            file_name_without_ext,
            folder_path,
            folder_name,
            extension,
            timestamp: now.to_rfc3339(),
            date: now.format("%Y-%m-%d").to_string(),
            time: now.format("%H:%M:%S").to_string(),
            automation_id: automation_id.to_string(),
            job_id: job_id.to_string(),
            attempt: 1,
        }
    }
}

pub fn expand_variables(input: &str, context: &EventContext) -> Result<String, AppError> {
    let attempt = context.attempt.to_string();
    let values = [
        ("{file_path}", context.file_path.as_str()),
        ("{file_name}", context.file_name.as_str()),
        ("{file_name_without_ext}", context.file_name_without_ext.as_str()),
        ("{folder_path}", context.folder_path.as_str()),
        ("{folder_name}", context.folder_name.as_str()),
        ("{extension}", context.extension.as_str()),
        ("{timestamp}", context.timestamp.as_str()),
        ("{date}", context.date.as_str()),
        ("{time}", context.time.as_str()),
        ("{automation_id}", context.automation_id.as_str()),
        ("{job_id}", context.job_id.as_str()),
        ("{attempt}", attempt.as_str()),
    ];
    let mut output = input.to_string();
    for (key, value) in values {
        output = output.replace(key, value);
    }
    if output.contains('{') {
        return Err(AppError::Validation("unknown variable".into()));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_event_values() {
        let context = EventContext {
            file_name: "report.csv".into(),
            attempt: 2,
            ..Default::default()
        };
        assert_eq!(
            expand_variables("{file_name}:{attempt}", &context).unwrap(),
            "report.csv:2"
        );
    }
}
