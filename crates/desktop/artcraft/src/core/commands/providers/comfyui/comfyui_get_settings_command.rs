use log::info;
use serde_derive::Serialize;
use tauri::State;

use crate::core::commands::response::shorthand::ResponseOrErrorMessage;
use crate::core::commands::response::success_response_wrapper::SerializeMarker;
use crate::core::state::comfyui_settings::{ComfyUiSettings, ComfyUiSettingsStore};

#[derive(Serialize)]
pub struct ComfyUiGetSettingsResponse {
  pub settings: ComfyUiSettings,
}

impl SerializeMarker for ComfyUiGetSettingsResponse {}

#[tauri::command]
pub async fn comfyui_get_settings_command(
  settings_store: State<'_, ComfyUiSettingsStore>,
) -> ResponseOrErrorMessage<ComfyUiGetSettingsResponse> {
  info!("comfyui_get_settings_command called");
  Ok(ComfyUiGetSettingsResponse { settings: settings_store.get() }.into())
}
