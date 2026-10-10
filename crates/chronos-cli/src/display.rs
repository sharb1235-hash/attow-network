use crate::CliError;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Envelope {
    pub schema: u32,
    pub command: String,
    pub status: String,
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}
pub struct Output {
    pub envelope: Envelope,
    pub exit_code: u8,
}
impl Output {
    pub fn success(command: &str, data: serde_json::Value) -> Self {
        Self {
            envelope: Envelope {
                schema: 1,
                command: command.into(),
                status: "success".into(),
                data,
                error: None,
            },
            exit_code: 0,
        }
    }
    pub fn error(command: &str, error: CliError) -> Self {
        let category = match error {
            CliError::Input(_) => "input",
            CliError::Infrastructure(_) => "infrastructure",
            CliError::Conflict(_) => "conflict",
            CliError::Unknown(_) => "unknown",
        };
        Self {
            exit_code: error.exit_code(),
            envelope: Envelope {
                schema: 1,
                command: command.into(),
                status: category.into(),
                data: serde_json::Value::Null,
                error: Some(serde_json::json!({"category":category,"message":error.to_string()})),
            },
        }
    }
    pub fn render(&self, json: bool) -> Result<String, CliError> {
        if json {
            return Ok(serde_json::to_string(&self.envelope)?);
        }
        if let Some(error) = &self.envelope.error {
            return Ok(format!(
                "{}: {}",
                self.envelope.status,
                error["message"].as_str().unwrap_or("command failed")
            ));
        }
        if let Some(help) = self
            .envelope
            .data
            .get("help")
            .and_then(serde_json::Value::as_str)
        {
            return Ok(help.to_owned());
        }
        Ok(format!(
            "Attow Network — {} ({})\n{}",
            self.envelope.command,
            self.envelope.status,
            serde_json::to_string_pretty(&self.envelope.data)?
        ))
    }
}
