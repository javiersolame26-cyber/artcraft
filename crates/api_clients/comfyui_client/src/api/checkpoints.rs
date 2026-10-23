use serde_json::Value;

use crate::error::ComfyUiError;

const CHECKPOINT_NODE: &str = "CheckpointLoaderSimple";
const CHECKPOINT_INPUT: &str = "ckpt_name";

/// Read the checkpoint file names from `GET /object_info/CheckpointLoaderSimple`.
///
/// Older servers describe a combo input as `[["a.safetensors", ...], {..}]`;
/// newer ones as `["COMBO", {"options": ["a.safetensors", ...]}]`. Both are accepted.
pub fn parse_checkpoint_names(body: &str) -> Result<Vec<String>, ComfyUiError> {
  let value: Value = serde_json::from_str(body)
    .map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })?;

  let spec = value
    .get(CHECKPOINT_NODE)
    .and_then(|node| node.get("input"))
    .and_then(|input| input.get("required"))
    .and_then(|required| required.get(CHECKPOINT_INPUT));

  let Some(spec) = spec else {
    return Ok(Vec::new());
  };

  let options = match spec.get(0) {
    Some(Value::Array(names)) => names.clone(),
    Some(Value::String(kind)) if kind == "COMBO" => spec
      .get(1)
      .and_then(|config| config.get("options"))
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default(),
    _ => Vec::new(),
  };

  Ok(options.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn legacy_combo_format() {
    let body = r#"{"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [["flux1-schnell-fp8.safetensors", "sd_xl_base_1.0.safetensors"], {"tooltip": "x"}]}}}}"#;
    assert_eq!(parse_checkpoint_names(body).unwrap(), vec!["flux1-schnell-fp8.safetensors", "sd_xl_base_1.0.safetensors"]);
  }

  #[test]
  fn new_combo_format() {
    let body = r#"{"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": ["COMBO", {"options": ["juggernaut.safetensors"]}]}}}}"#;
    assert_eq!(parse_checkpoint_names(body).unwrap(), vec!["juggernaut.safetensors"]);
  }

  #[test]
  fn no_checkpoints_installed() {
    let body = r#"{"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [[], {}]}}}}"#;
    assert!(parse_checkpoint_names(body).unwrap().is_empty());
  }
}
