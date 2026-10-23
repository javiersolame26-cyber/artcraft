use comfyui_client::api::system_stats::DeviceInfo;
use comfyui_client::client::ComfyUiClient;
use comfyui_client::error::ComfyUiError;
use log::{info, warn};
use serde_derive::{Deserialize, Serialize};
use tauri::State;

use crate::core::commands::response::shorthand::ResponseOrErrorMessage;
use crate::core::commands::response::success_response_wrapper::SerializeMarker;
use crate::core::state::comfyui_settings::ComfyUiSettingsStore;

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Deserialize)]
pub struct ComfyUiTestConnectionRequest {
  /// Test an address before saving it. Defaults to the saved address.
  pub maybe_server_url: Option<String>,
}

#[derive(Serialize)]
pub struct ComfyUiTestConnectionResponse {
  pub server_url: String,
  pub maybe_comfyui_version: Option<String>,
  pub devices: Vec<ComfyUiDevice>,
  /// Every checkpoint file in ComfyUI's `models/checkpoints` folder.
  pub checkpoints: Vec<String>,
  /// Configured checkpoints that ComfyUI does not have.
  pub missing_checkpoints: Vec<String>,
}

#[derive(Serialize)]
pub struct ComfyUiDevice {
  pub name: String,
  pub device_type: String,
  pub maybe_vram_total_gib: Option<f64>,
}

impl SerializeMarker for ComfyUiTestConnectionResponse {}

#[tauri::command]
pub async fn comfyui_test_connection_command(
  request: ComfyUiTestConnectionRequest,
  settings_store: State<'_, ComfyUiSettingsStore>,
) -> ResponseOrErrorMessage<ComfyUiTestConnectionResponse> {
  let settings = settings_store.get();
  let server_url = request.maybe_server_url
    .map(|url| url.trim().to_string())
    .filter(|url| !url.is_empty())
    .unwrap_or_else(|| settings.server_url.clone());

  info!("comfyui_test_connection_command called for {}", server_url);

  let client = ComfyUiClient::new(&server_url)
    .map_err(|_| format!("\"{}\" is not a valid ComfyUI address. Use something like http://127.0.0.1:8188", server_url))?;

  let stats = client.get_system_stats().await.map_err(|err| describe_connection_error(&server_url, &err))?;
  let checkpoints = client.list_checkpoints().await.map_err(|err| describe_connection_error(&server_url, &err))?;

  let missing_checkpoints = settings.configured_checkpoints()
    .iter()
    .filter(|name| !checkpoints.iter().any(|installed| installed == *name))
    .map(|name| name.to_string())
    .collect();

  Ok(ComfyUiTestConnectionResponse {
    server_url: client.base_url().to_string(),
    maybe_comfyui_version: stats.maybe_comfyui_version,
    devices: stats.devices.into_iter().map(to_device).collect(),
    checkpoints,
    missing_checkpoints,
  }.into())
}

fn describe_connection_error(server_url: &str, err: &ComfyUiError) -> String {
  warn!("ComfyUI connection test failed for {}: {:?}", server_url, err);
  if err.is_unreachable() {
    format!("Could not reach ComfyUI at {}. Make sure ComfyUI is running and the address and port are right.", server_url)
  } else {
    format!("ComfyUI at {} answered with an error: {}", server_url, err)
  }
}

fn to_device(device: DeviceInfo) -> ComfyUiDevice {
  ComfyUiDevice {
    name: device.name,
    device_type: device.device_type,
    maybe_vram_total_gib: device.maybe_vram_total_bytes
      .filter(|bytes| *bytes > 0)
      .map(|bytes| (bytes as f64 / BYTES_PER_GIB * 10.0).round() / 10.0),
  }
}
