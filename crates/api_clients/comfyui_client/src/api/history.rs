use std::collections::BTreeMap;

use serde_derive::Deserialize;
use serde_json::Value;

use crate::error::ComfyUiError;

const OUTPUT_FOLDER_TYPE: &str = "output";

/// A prompt that ComfyUI has finished with, from `GET /history/{prompt_id}`.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptHistory {
  pub status: PromptStatus,

  /// Saved images, in node order. Preview (temp) images are excluded when saved ones exist.
  pub output_images: Vec<OutputImage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptStatus {
  Success,
  Error { message: String },
  /// ComfyUI kept a history entry but did not mark it completed (e.g. interrupted).
  Incomplete { status: String },
}

/// One image written by a SaveImage/PreviewImage node.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OutputImage {
  pub filename: String,

  #[serde(default)]
  pub subfolder: String,

  /// "output" for saved images, "temp" for previews.
  #[serde(rename = "type", default = "default_folder_type")]
  pub folder_type: String,
}

#[derive(Deserialize)]
struct RawHistoryEntry {
  #[serde(default)]
  outputs: BTreeMap<String, RawNodeOutput>,
  status: Option<RawStatus>,
}

#[derive(Deserialize)]
struct RawNodeOutput {
  #[serde(default)]
  images: Vec<OutputImage>,
}

#[derive(Deserialize)]
struct RawStatus {
  status_str: Option<String>,
  #[serde(default)]
  completed: bool,
  #[serde(default)]
  messages: Vec<Value>,
}

/// Parse a `/history/{prompt_id}` body. Returns `None` while the prompt has no
/// history yet (still queued or running, or unknown to this server).
pub fn parse_history(body: &str, prompt_id: &str) -> Result<Option<PromptHistory>, ComfyUiError> {
  let mut entries: BTreeMap<String, RawHistoryEntry> = serde_json::from_str(body)
    .map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })?;

  let Some(entry) = entries.remove(prompt_id) else {
    return Ok(None);
  };

  let status = match entry.status {
    None => PromptStatus::Incomplete { status: "unknown".to_string() },
    Some(raw) => parse_status(raw),
  };

  Ok(Some(PromptHistory {
    status,
    output_images: collect_output_images(entry.outputs),
  }))
}

fn parse_status(raw: RawStatus) -> PromptStatus {
  let status_str = raw.status_str.unwrap_or_default();
  if status_str == "error" {
    return PromptStatus::Error { message: error_message(&raw.messages) };
  }
  if raw.completed || status_str == "success" {
    return PromptStatus::Success;
  }
  if raw.messages.iter().any(|m| message_kind(m) == Some("execution_interrupted")) {
    return PromptStatus::Error { message: "The generation was interrupted in ComfyUI.".to_string() };
  }
  PromptStatus::Incomplete { status: status_str }
}

fn collect_output_images(outputs: BTreeMap<String, RawNodeOutput>) -> Vec<OutputImage> {
  let all: Vec<OutputImage> = outputs.into_values()
    .flat_map(|output| output.images)
    .collect();
  let saved: Vec<OutputImage> = all.iter()
    .filter(|image| image.folder_type == OUTPUT_FOLDER_TYPE)
    .cloned()
    .collect();
  if saved.is_empty() { all } else { saved }
}

/// ComfyUI reports failures as `["execution_error", { "exception_message": ..., "node_type": ... }]`.
fn error_message(messages: &[Value]) -> String {
  messages.iter()
    .filter(|m| message_kind(m) == Some("execution_error"))
    .filter_map(|m| m.get(1))
    .map(|details| {
      let exception = details.get("exception_message").and_then(Value::as_str).unwrap_or("unknown error").trim();
      match details.get("node_type").and_then(Value::as_str) {
        Some(node) => format!("{} (node {})", exception, node),
        None => exception.to_string(),
      }
    })
    .next()
    .unwrap_or_else(|| "ComfyUI reported an error without details.".to_string())
}

fn message_kind(message: &Value) -> Option<&str> {
  message.get(0).and_then(Value::as_str)
}

fn default_folder_type() -> String {
  OUTPUT_FOLDER_TYPE.to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  const PROMPT_ID: &str = "4f8a2c1e-0000-4000-8000-000000000001";

  #[test]
  fn missing_prompt_is_none() {
    assert_eq!(parse_history("{}", PROMPT_ID).unwrap(), None);
  }

  #[test]
  fn success_collects_saved_images_in_node_order() {
    let body = format!(r#"{{"{PROMPT_ID}": {{
      "outputs": {{
        "9": {{"images": [{{"filename": "b.png", "subfolder": "ArtCraft", "type": "output"}}]}},
        "12": {{"images": [{{"filename": "preview.png", "subfolder": "", "type": "temp"}}]}},
        "10": {{"images": [{{"filename": "a.png", "subfolder": "ArtCraft", "type": "output"}}]}}
      }},
      "status": {{"status_str": "success", "completed": true, "messages": []}}
    }}}}"#);
    let history = parse_history(&body, PROMPT_ID).unwrap().unwrap();
    assert_eq!(history.status, PromptStatus::Success);
    let names: Vec<&str> = history.output_images.iter().map(|i| i.filename.as_str()).collect();
    assert_eq!(names, vec!["a.png", "b.png"]);
  }

  #[test]
  fn previews_are_used_when_nothing_was_saved() {
    let body = format!(r#"{{"{PROMPT_ID}": {{
      "outputs": {{"5": {{"images": [{{"filename": "p.png", "type": "temp"}}]}}}},
      "status": {{"status_str": "success", "completed": true}}
    }}}}"#);
    let history = parse_history(&body, PROMPT_ID).unwrap().unwrap();
    assert_eq!(history.output_images.len(), 1);
    assert_eq!(history.output_images[0].subfolder, "");
  }

  #[test]
  fn error_reports_exception_and_node() {
    let body = format!(r#"{{"{PROMPT_ID}": {{
      "outputs": {{}},
      "status": {{"status_str": "error", "completed": false, "messages": [
        ["execution_start", {{"prompt_id": "{PROMPT_ID}"}}],
        ["execution_error", {{"exception_message": "Allocation on device \n", "node_type": "KSampler"}}]
      ]}}
    }}}}"#);
    let history = parse_history(&body, PROMPT_ID).unwrap().unwrap();
    assert_eq!(history.status, PromptStatus::Error { message: "Allocation on device (node KSampler)".to_string() });
  }

  #[test]
  fn interrupted_is_an_error() {
    let body = format!(r#"{{"{PROMPT_ID}": {{
      "outputs": {{}},
      "status": {{"status_str": "", "completed": false, "messages": [["execution_interrupted", {{}}]]}}
    }}}}"#);
    let history = parse_history(&body, PROMPT_ID).unwrap().unwrap();
    assert!(matches!(history.status, PromptStatus::Error { .. }));
  }

  #[test]
  fn malformed_body_is_a_parse_error() {
    assert!(matches!(parse_history("[1, 2]", PROMPT_ID), Err(ComfyUiError::Parse { .. })));
  }
}
