use serde_derive::Deserialize;

use crate::error::ComfyUiError;

/// Server and GPU information from `GET /system_stats`.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemStats {
  pub maybe_comfyui_version: Option<String>,
  pub devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DeviceInfo {
  /// e.g. "cuda:0 NVIDIA GeForce RTX 3060 : cudaMallocAsync"
  pub name: String,

  /// e.g. "cuda", "cpu", "mps"
  #[serde(rename = "type", default)]
  pub device_type: String,

  #[serde(rename = "vram_total", default)]
  pub maybe_vram_total_bytes: Option<u64>,

  #[serde(rename = "vram_free", default)]
  pub maybe_vram_free_bytes: Option<u64>,
}

#[derive(Deserialize)]
struct RawSystemStats {
  system: Option<RawSystem>,
  #[serde(default)]
  devices: Vec<DeviceInfo>,
}

#[derive(Deserialize)]
struct RawSystem {
  comfyui_version: Option<String>,
}

pub fn parse_system_stats(body: &str) -> Result<SystemStats, ComfyUiError> {
  let raw: RawSystemStats = serde_json::from_str(body)
    .map_err(|error| ComfyUiError::Parse { error, body: body.to_string() })?;
  Ok(SystemStats {
    maybe_comfyui_version: raw.system.and_then(|s| s.comfyui_version),
    devices: raw.devices,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_version_and_gpu() {
    let body = r#"{
      "system": {"os": "nt", "python_version": "3.12.7", "comfyui_version": "0.3.68", "embedded_python": true},
      "devices": [{"name": "cuda:0 NVIDIA GeForce RTX 3060 : cudaMallocAsync", "type": "cuda", "index": 0,
                   "vram_total": 12884377600, "vram_free": 11811160064, "torch_vram_total": 0, "torch_vram_free": 0}]
    }"#;
    let stats = parse_system_stats(body).unwrap();
    assert_eq!(stats.maybe_comfyui_version.as_deref(), Some("0.3.68"));
    assert_eq!(stats.devices[0].device_type, "cuda");
    assert_eq!(stats.devices[0].maybe_vram_total_bytes, Some(12884377600));
  }

  #[test]
  fn older_servers_without_version() {
    let stats = parse_system_stats(r#"{"system": {"os": "posix"}, "devices": []}"#).unwrap();
    assert_eq!(stats.maybe_comfyui_version, None);
    assert!(stats.devices.is_empty());
  }
}
