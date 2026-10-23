use comfyui_client::client::ComfyUiClient;
use log::{error, info};
use serde_derive::{Deserialize, Serialize};
use tauri::State;

use crate::core::commands::response::shorthand::ResponseOrErrorMessage;
use crate::core::commands::response::success_response_wrapper::SerializeMarker;
use crate::core::state::comfyui_settings::{ComfyUiSettings, ComfyUiSettingsStore};
use crate::core::state::data_dir::app_data_root::AppDataRoot;

#[derive(Deserialize)]
pub struct ComfyUiUpdateSettingsRequest {
  pub settings: ComfyUiSettings,
}

#[derive(Serialize)]
pub struct ComfyUiUpdateSettingsResponse {
  /// The settings as stored (trimmed, defaults restored, strength clamped).
  pub settings: ComfyUiSettings,
}

impl SerializeMarker for ComfyUiUpdateSettingsResponse {}

#[tauri::command]
pub async fn comfyui_update_settings_command(
  request: ComfyUiUpdateSettingsRequest,
  settings_store: State<'_, ComfyUiSettingsStore>,
  app_data_root: State<'_, AppDataRoot>,
) -> ResponseOrErrorMessage<ComfyUiUpdateSettingsResponse> {
  info!("comfyui_update_settings_command called");

  let settings = request.settings.normalized();

  if ComfyUiClient::new(&settings.server_url).is_err() {
    return Err(format!("\"{}\" is not a valid ComfyUI address. Use something like http://127.0.0.1:8188", settings.server_url).into());
  }

  settings.persist(&app_data_root).map_err(|err| {
    error!("Failed to save ComfyUI settings: {:?}", err);
    "Failed to save the ComfyUI settings"
  })?;

  settings_store.replace(settings.clone());

  Ok(ComfyUiUpdateSettingsResponse { settings }.into())
}
