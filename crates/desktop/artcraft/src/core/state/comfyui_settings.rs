use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, RwLock};

use comfyui_client::api::model_files::ModelFolder;
use comfyui_client::client::DEFAULT_COMFYUI_URL;
use comfyui_client::workflows::kontext_workflow::KontextModelFiles;
use comfyui_client::workflows::model_family::ModelFamily;
use errors::AnyhowResult;
use log::{info, warn};
use serde_derive::{Deserialize, Serialize};

use crate::core::state::data_dir::app_data_root::AppDataRoot;

/// Comfy-Org's all-in-one FP8 checkpoints load with a plain `Load Checkpoint`
/// node, so they are the defaults ComfyUI's own examples use.
pub const DEFAULT_FLUX_SCHNELL_CHECKPOINT: &str = "flux1-schnell-fp8.safetensors";
pub const DEFAULT_FLUX_DEV_CHECKPOINT: &str = "flux1-dev-fp8.safetensors";
pub const DEFAULT_SDXL_CHECKPOINT: &str = "sd_xl_base_1.0.safetensors";

/// FLUX.1 Kontext [dev] ships as separate files; these are the names in
/// ComfyUI's Kontext template (Comfy-Org and comfyanonymous on Hugging Face).
pub const DEFAULT_KONTEXT_DIFFUSION_MODEL: &str = "flux1-dev-kontext_fp8_scaled.safetensors";
pub const DEFAULT_KONTEXT_CLIP_L: &str = "clip_l.safetensors";
pub const DEFAULT_KONTEXT_T5XXL: &str = "t5xxl_fp8_e4m3fn_scaled.safetensors";
pub const DEFAULT_KONTEXT_VAE: &str = "ae.safetensors";

/// How much an input image (canvas, 3D scene, reference) may change.
const DEFAULT_IMAGE_EDIT_STRENGTH: f32 = 0.65;
const MIN_IMAGE_EDIT_STRENGTH: f32 = 0.05;

/// Where the user's ComfyUI server lives and which model files each local
/// model uses. Stored as `settings/comfyui_settings.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComfyUiSettings {
  pub server_url: String,
  pub flux_schnell_checkpoint: String,
  pub flux_dev_checkpoint: String,
  pub sdxl_checkpoint: String,
  /// In `models/diffusion_models`.
  pub kontext_diffusion_model: String,
  /// In `models/text_encoders`.
  pub kontext_clip_l: String,
  /// In `models/text_encoders`.
  pub kontext_t5xxl: String,
  /// In `models/vae`.
  pub kontext_vae: String,
  /// 0.05–1.0. Lower keeps more of the input image in image-to-image.
  pub image_edit_strength: f32,
}

/// Shared, live copy of the settings for commands and generation.
#[derive(Clone)]
pub struct ComfyUiSettingsStore {
  settings: Arc<RwLock<ComfyUiSettings>>,
}

impl Default for ComfyUiSettings {
  fn default() -> Self {
    Self {
      server_url: DEFAULT_COMFYUI_URL.to_string(),
      flux_schnell_checkpoint: DEFAULT_FLUX_SCHNELL_CHECKPOINT.to_string(),
      flux_dev_checkpoint: DEFAULT_FLUX_DEV_CHECKPOINT.to_string(),
      sdxl_checkpoint: DEFAULT_SDXL_CHECKPOINT.to_string(),
      kontext_diffusion_model: DEFAULT_KONTEXT_DIFFUSION_MODEL.to_string(),
      kontext_clip_l: DEFAULT_KONTEXT_CLIP_L.to_string(),
      kontext_t5xxl: DEFAULT_KONTEXT_T5XXL.to_string(),
      kontext_vae: DEFAULT_KONTEXT_VAE.to_string(),
      image_edit_strength: DEFAULT_IMAGE_EDIT_STRENGTH,
    }
  }
}

impl ComfyUiSettings {
  /// Missing or unreadable settings fall back to the defaults; generation
  /// then reports a clear error if the server isn't there.
  pub fn load_or_default(app_data_root: &AppDataRoot) -> Self {
    let path = app_data_root.settings_dir().get_comfyui_settings_path();
    if !path.exists() {
      return Self::default();
    }
    match std::fs::read_to_string(&path).map(|contents| serde_json::from_str::<Self>(&contents)) {
      Ok(Ok(settings)) => settings.normalized(),
      Ok(Err(err)) => {
        warn!("Ignoring unreadable ComfyUI settings at {:?}: {:?}", path, err);
        Self::default()
      }
      Err(err) => {
        warn!("Could not read ComfyUI settings at {:?}: {:?}", path, err);
        Self::default()
      }
    }
  }

  pub fn persist(&self, app_data_root: &AppDataRoot) -> AnyhowResult<()> {
    let path = app_data_root.settings_dir().get_comfyui_settings_path();
    let json = serde_json::to_string_pretty(self)?;
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(&path)?;
    file.write_all(json.as_bytes())?;
    file.flush()?;
    info!("Saved ComfyUI settings to {:?}", path);
    Ok(())
  }

  /// Trim user input, restore defaults for blank fields and clamp the strength.
  pub fn normalized(self) -> Self {
    let defaults = Self::default();
    Self {
      server_url: non_blank_or(self.server_url, defaults.server_url),
      flux_schnell_checkpoint: non_blank_or(self.flux_schnell_checkpoint, defaults.flux_schnell_checkpoint),
      flux_dev_checkpoint: non_blank_or(self.flux_dev_checkpoint, defaults.flux_dev_checkpoint),
      sdxl_checkpoint: non_blank_or(self.sdxl_checkpoint, defaults.sdxl_checkpoint),
      kontext_diffusion_model: non_blank_or(self.kontext_diffusion_model, defaults.kontext_diffusion_model),
      kontext_clip_l: non_blank_or(self.kontext_clip_l, defaults.kontext_clip_l),
      kontext_t5xxl: non_blank_or(self.kontext_t5xxl, defaults.kontext_t5xxl),
      kontext_vae: non_blank_or(self.kontext_vae, defaults.kontext_vae),
      image_edit_strength: if self.image_edit_strength.is_finite() {
        self.image_edit_strength.clamp(MIN_IMAGE_EDIT_STRENGTH, 1.0)
      } else {
        defaults.image_edit_strength
      },
    }
  }

  pub fn checkpoint_for(&self, family: ModelFamily) -> &str {
    match family {
      ModelFamily::FluxSchnell => &self.flux_schnell_checkpoint,
      ModelFamily::FluxDev => &self.flux_dev_checkpoint,
      ModelFamily::Sdxl => &self.sdxl_checkpoint,
    }
  }

  /// Every checkpoint name the local models are configured to use.
  pub fn configured_checkpoints(&self) -> [&str; 3] {
    [&self.flux_schnell_checkpoint, &self.flux_dev_checkpoint, &self.sdxl_checkpoint]
  }

  pub fn kontext_files(&self) -> KontextModelFiles<'_> {
    KontextModelFiles {
      diffusion_model: &self.kontext_diffusion_model,
      clip_l: &self.kontext_clip_l,
      t5xxl: &self.kontext_t5xxl,
      vae: &self.kontext_vae,
    }
  }

  /// The Kontext files with the ComfyUI folder each one must be in.
  pub fn kontext_files_by_folder(&self) -> [(ModelFolder, &str); 4] {
    [
      (ModelFolder::DiffusionModels, &self.kontext_diffusion_model),
      (ModelFolder::TextEncoders, &self.kontext_clip_l),
      (ModelFolder::TextEncoders, &self.kontext_t5xxl),
      (ModelFolder::Vae, &self.kontext_vae),
    ]
  }
}

impl ComfyUiSettingsStore {
  pub fn new(settings: ComfyUiSettings) -> Self {
    Self { settings: Arc::new(RwLock::new(settings)) }
  }

  pub fn get(&self) -> ComfyUiSettings {
    match self.settings.read() {
      Ok(settings) => settings.clone(),
      Err(poisoned) => poisoned.into_inner().clone(),
    }
  }

  pub fn replace(&self, settings: ComfyUiSettings) {
    match self.settings.write() {
      Ok(mut current) => *current = settings,
      Err(poisoned) => *poisoned.into_inner() = settings,
    }
  }
}

fn non_blank_or(value: String, fallback: String) -> String {
  let trimmed = value.trim();
  if trimmed.is_empty() { fallback } else { trimmed.to_string() }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn missing_fields_use_defaults() {
    let settings: ComfyUiSettings = serde_json::from_str(r#"{"server_url": "http://192.168.1.20:8188"}"#).unwrap();
    assert_eq!(settings.server_url, "http://192.168.1.20:8188");
    assert_eq!(settings.flux_schnell_checkpoint, DEFAULT_FLUX_SCHNELL_CHECKPOINT);
    assert_eq!(settings.kontext_diffusion_model, DEFAULT_KONTEXT_DIFFUSION_MODEL);
    assert_eq!(settings.image_edit_strength, DEFAULT_IMAGE_EDIT_STRENGTH);
  }

  #[test]
  fn normalized_trims_blanks_and_clamps() {
    let settings = ComfyUiSettings {
      server_url: "  ".to_string(),
      flux_schnell_checkpoint: " juggernaut.safetensors ".to_string(),
      flux_dev_checkpoint: String::new(),
      kontext_vae: " ".to_string(),
      image_edit_strength: 3.0,
      ..ComfyUiSettings::default()
    }.normalized();

    assert_eq!(settings.server_url, DEFAULT_COMFYUI_URL);
    assert_eq!(settings.flux_schnell_checkpoint, "juggernaut.safetensors");
    assert_eq!(settings.flux_dev_checkpoint, DEFAULT_FLUX_DEV_CHECKPOINT);
    assert_eq!(settings.kontext_vae, DEFAULT_KONTEXT_VAE);
    assert_eq!(settings.image_edit_strength, 1.0);
  }

  #[test]
  fn non_finite_strength_uses_default() {
    let settings = ComfyUiSettings { image_edit_strength: f32::NAN, ..ComfyUiSettings::default() }.normalized();
    assert_eq!(settings.image_edit_strength, DEFAULT_IMAGE_EDIT_STRENGTH);
  }

  #[test]
  fn checkpoint_per_family() {
    let settings = ComfyUiSettings::default();
    assert_eq!(settings.checkpoint_for(ModelFamily::FluxDev), DEFAULT_FLUX_DEV_CHECKPOINT);
    assert_eq!(settings.checkpoint_for(ModelFamily::Sdxl), DEFAULT_SDXL_CHECKPOINT);
  }

  #[test]
  fn kontext_files_keep_their_folders() {
    let settings = ComfyUiSettings::default();
    let files = settings.kontext_files_by_folder();
    assert_eq!(files[0], (ModelFolder::DiffusionModels, DEFAULT_KONTEXT_DIFFUSION_MODEL));
    assert_eq!(files[2], (ModelFolder::TextEncoders, DEFAULT_KONTEXT_T5XXL));
    assert_eq!(files[3], (ModelFolder::Vae, DEFAULT_KONTEXT_VAE));
    assert_eq!(settings.kontext_files().clip_l, DEFAULT_KONTEXT_CLIP_L);
  }
}
