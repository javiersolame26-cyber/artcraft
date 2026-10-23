use serde_json::Value;

use crate::error::ComfyUiError;

/// A ComfyUI `models/…` folder, read through the loader node that lists it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ModelFolder {
  /// `models/checkpoints`: all-in-one files (model, text encoders and VAE).
  Checkpoints,
  /// `models/diffusion_models` (formerly `unet`): the diffusion model alone.
  DiffusionModels,
  /// `models/text_encoders` (formerly `clip`).
  TextEncoders,
  /// `models/vae`.
  Vae,
}

impl ModelFolder {
  /// The loader node whose `/object_info` entry lists this folder.
  pub fn loader_node(self) -> &'static str {
    self.loader().0
  }

  fn loader(self) -> (&'static str, &'static str) {
    match self {
      Self::Checkpoints => ("CheckpointLoaderSimple", "ckpt_name"),
      Self::DiffusionModels => ("UNETLoader", "unet_name"),
      Self::TextEncoders => ("DualCLIPLoader", "clip_name1"),
      Self::Vae => ("VAELoader", "vae_name"),
    }
  }
}

/// Read the file names in `folder` from `GET /object_info/{loader node}`.
///
/// Older servers describe a combo input as `[["a.safetensors", ...], {..}]`;
/// newer ones as `["COMBO", {"options": ["a.safetensors", ...]}]`. Both are accepted.
pub fn parse_model_file_names(body: &str, folder: ModelFolder) -> Result<Vec<String>, ComfyUiError> {
  let value: Value = serde_json::from_str(body)
    .map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })?;

  let (node, input) = folder.loader();
  let spec = value
    .get(node)
    .and_then(|node| node.get("input"))
    .and_then(|inputs| inputs.get("required"))
    .and_then(|required| required.get(input));

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
    assert_eq!(
      parse_model_file_names(body, ModelFolder::Checkpoints).unwrap(),
      vec!["flux1-schnell-fp8.safetensors", "sd_xl_base_1.0.safetensors"],
    );
  }

  #[test]
  fn new_combo_format() {
    let body = r#"{"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": ["COMBO", {"options": ["juggernaut.safetensors"]}]}}}}"#;
    assert_eq!(parse_model_file_names(body, ModelFolder::Checkpoints).unwrap(), vec!["juggernaut.safetensors"]);
  }

  #[test]
  fn no_files_installed() {
    let body = r#"{"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [[], {}]}}}}"#;
    assert!(parse_model_file_names(body, ModelFolder::Checkpoints).unwrap().is_empty());
  }

  #[test]
  fn reads_each_folder_from_its_loader() {
    let body = r#"{
      "UNETLoader": {"input": {"required": {"unet_name": [["flux1-dev-kontext_fp8_scaled.safetensors"]], "weight_dtype": [["default"]]}}},
      "DualCLIPLoader": {"input": {"required": {"clip_name1": [["clip_l.safetensors", "t5xxl_fp8_e4m3fn_scaled.safetensors"]], "clip_name2": [["clip_l.safetensors"]]}}},
      "VAELoader": {"input": {"required": {"vae_name": [["ae.safetensors", "pixel_space"]]}}}
    }"#;
    assert_eq!(
      parse_model_file_names(body, ModelFolder::DiffusionModels).unwrap(),
      vec!["flux1-dev-kontext_fp8_scaled.safetensors"],
    );
    assert_eq!(
      parse_model_file_names(body, ModelFolder::TextEncoders).unwrap(),
      vec!["clip_l.safetensors", "t5xxl_fp8_e4m3fn_scaled.safetensors"],
    );
    assert_eq!(parse_model_file_names(body, ModelFolder::Vae).unwrap(), vec!["ae.safetensors", "pixel_space"]);
  }

  #[test]
  fn unknown_node_lists_nothing() {
    assert!(parse_model_file_names("{}", ModelFolder::Vae).unwrap().is_empty());
  }
}
