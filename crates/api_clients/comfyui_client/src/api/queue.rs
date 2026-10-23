use serde_derive::Deserialize;
use serde_json::Value;

use crate::error::ComfyUiError;

/// Prompt IDs ComfyUI is running or still has waiting, from `GET /queue`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
  pub running: Vec<String>,
  pub pending: Vec<String>,
}

impl QueueSnapshot {
  pub fn contains(&self, prompt_id: &str) -> bool {
    self.running.iter().chain(self.pending.iter()).any(|id| id == prompt_id)
  }
}

/// Queue items are positional arrays: `[number, prompt_id, prompt, extra_data, outputs]`.
#[derive(Deserialize)]
struct RawQueue {
  #[serde(default)]
  queue_running: Vec<Vec<Value>>,
  #[serde(default)]
  queue_pending: Vec<Vec<Value>>,
}

pub fn parse_queue(body: &str) -> Result<QueueSnapshot, ComfyUiError> {
  let raw: RawQueue = serde_json::from_str(body)
    .map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })?;
  Ok(QueueSnapshot {
    running: prompt_ids(&raw.queue_running),
    pending: prompt_ids(&raw.queue_pending),
  })
}

fn prompt_ids(items: &[Vec<Value>]) -> Vec<String> {
  items.iter()
    .filter_map(|item| item.get(1).and_then(Value::as_str))
    .map(str::to_string)
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_running_and_pending_ids() {
    let body = r#"{
      "queue_running": [[3, "run-1", {}, {}, ["9"]]],
      "queue_pending": [[4, "wait-1", {}, {}, ["9"]], [5, "wait-2", {}, {}, ["9"]]]
    }"#;
    let queue = parse_queue(body).unwrap();
    assert_eq!(queue.running, vec!["run-1"]);
    assert_eq!(queue.pending, vec!["wait-1", "wait-2"]);
    assert!(queue.contains("wait-2"));
    assert!(!queue.contains("gone"));
  }

  #[test]
  fn empty_queue() {
    assert_eq!(parse_queue(r#"{"queue_running": [], "queue_pending": []}"#).unwrap(), QueueSnapshot::default());
  }
}
