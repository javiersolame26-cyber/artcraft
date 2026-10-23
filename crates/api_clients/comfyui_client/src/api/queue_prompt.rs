use serde_derive::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ComfyUiError;

/// Body of `POST /prompt`.
#[derive(Serialize)]
pub struct QueuePromptRequest<'a> {
  pub prompt: &'a Value,
  pub client_id: &'a str,
}

/// A workflow ComfyUI accepted into its queue.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QueuedPrompt {
  pub prompt_id: String,
  #[serde(default)]
  pub number: Option<u64>,
}

#[derive(Deserialize)]
struct RawRejection {
  error: Option<RawRejectionError>,
  #[serde(default)]
  node_errors: Value,
}

#[derive(Deserialize)]
struct RawRejectionError {
  message: Option<String>,
  details: Option<String>,
}

pub fn parse_queued_prompt(body: &str) -> Result<QueuedPrompt, ComfyUiError> {
  serde_json::from_str(body).map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })
}

/// ComfyUI answers a rejected workflow with HTTP 400 and
/// `{"error": {"message", "details"}, "node_errors": {...}}`.
pub fn parse_rejection(status: u16, body: &str) -> ComfyUiError {
  let Ok(raw) = serde_json::from_str::<RawRejection>(body) else {
    return ComfyUiError::BadStatus { status, body: body.to_string() };
  };
  let message = match raw.error {
    Some(RawRejectionError { message: Some(message), details: Some(details) }) if !details.is_empty() => {
      format!("{}: {}", message, details)
    }
    Some(RawRejectionError { message: Some(message), .. }) => message,
    _ => format!("HTTP {}", status),
  };
  let node_errors = node_error_summary(&raw.node_errors);
  let message = if node_errors.is_empty() { message } else { format!("{} ({})", message, node_errors) };
  ComfyUiError::PromptRejected { message, node_errors }
}

/// `node_errors` maps node ids to `{"errors": [{"message", "details"}], "class_type"}`.
fn node_error_summary(node_errors: &Value) -> String {
  let Some(nodes) = node_errors.as_object() else {
    return String::new();
  };
  nodes.values()
    .flat_map(|node| {
      let class_type = node.get("class_type").and_then(Value::as_str).unwrap_or("node").to_string();
      node.get("errors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(move |err| {
          let message = err.get("message").and_then(Value::as_str).unwrap_or("error");
          match err.get("details").and_then(Value::as_str).filter(|d| !d.is_empty()) {
            Some(details) => format!("{}: {} - {}", class_type, message, details),
            None => format!("{}: {}", class_type, message),
          }
        })
    })
    .collect::<Vec<_>>()
    .join("; ")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_accepted_prompt() {
    let queued = parse_queued_prompt(r#"{"prompt_id": "abc", "number": 7, "node_errors": {}}"#).unwrap();
    assert_eq!(queued, QueuedPrompt { prompt_id: "abc".to_string(), number: Some(7) });
  }

  #[test]
  fn rejection_names_missing_checkpoint() {
    let body = r#"{
      "error": {"type": "prompt_outputs_failed_validation", "message": "Prompt outputs failed validation", "details": "", "extra_info": {}},
      "node_errors": {"1": {"errors": [{"type": "value_not_in_list", "message": "Value not in list", "details": "ckpt_name: 'flux1-schnell-fp8.safetensors' not in []"}], "class_type": "CheckpointLoaderSimple"}}
    }"#;
    let ComfyUiError::PromptRejected { message, .. } = parse_rejection(400, body) else {
      panic!("expected a rejection");
    };
    assert!(message.starts_with("Prompt outputs failed validation"));
    assert!(message.contains("CheckpointLoaderSimple: Value not in list - ckpt_name: 'flux1-schnell-fp8.safetensors' not in []"));
  }

  #[test]
  fn non_json_rejection_keeps_the_body() {
    assert!(matches!(parse_rejection(500, "boom"), ComfyUiError::BadStatus { status: 500, .. }));
  }
}
