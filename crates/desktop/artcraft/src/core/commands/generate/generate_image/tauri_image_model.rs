use comfyui_client::workflows::model_family::ModelFamily;
use serde_derive::Deserialize;

/// Unified image model enum covering text-to-image, image edit, and inpainting.
///
/// This is used in the Tauri command bridge.
/// Don't change the serializations without coordinating with the frontend.
///
/// MIGRATION (2026-07): we're moving the frontend to send storyteller-web omni
/// identifiers (`CommonImageModel` serde strings). Variants whose legacy Tauri
/// id differs carry the omni id as a `#[serde(alias)]` so BOTH deserialize.
/// Once the frontend is 100% omni ids, the legacy renames can be dropped.
#[derive(Deserialize, Debug, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TauriImageModel {
  // Text-to-image models

  #[serde(rename = "flux_1_dev")]
  Flux1Dev,
  #[serde(rename = "flux_1_schnell")]
  Flux1Schnell,
  #[serde(rename = "flux_pro_11", alias = "flux_pro_1p1")]
  FluxPro11,
  #[serde(rename = "flux_pro_11_ultra", alias = "flux_pro_1p1_ultra")]
  FluxPro11Ultra,
  #[serde(rename = "grok_image", alias = "grok_imagine_image")]
  GrokImage,
  #[serde(rename = "recraft_3")]
  Recraft3,
  #[serde(rename = "gpt_image_1")]
  GptImage1,
  #[serde(rename = "gpt_image_1p5")]
  GptImage1p5,
  #[serde(rename = "gpt_image_2")]
  GptImage2,
  #[serde(rename = "gpt_image_2p5_flare")]
  GptImage2p5Flare,
  #[serde(rename = "gpt_image_2p5_sunburst")]
  GptImage2p5Sunburst,
  #[serde(rename = "gemini_25_flash")]
  Gemini25Flash,
  #[serde(rename = "nano_banana")]
  NanoBanana,
  #[serde(rename = "nano_banana_2")]
  NanoBanana2,
  #[serde(rename = "nano_banana_pro")]
  NanoBananaPro,
  #[serde(rename = "seedream_4")]
  Seedream4,
  #[serde(rename = "seedream_4p5")]
  Seedream4p5,
  #[serde(rename = "seedream_5_lite")]
  Seedream5Lite,
  // Generic Midjourney experience, served via the native Midjourney provider.
  #[serde(rename = "midjourney")]
  Midjourney,

  // Versioned Midjourney models, served by storyteller-web via the
  // Artcraft provider (dispatched through artcraft_router).
  #[serde(rename = "midjourney_7")]
  Midjourney7,
  #[serde(rename = "midjourney_7_niji")]
  Midjourney7Niji,
  #[serde(rename = "midjourney_8")]
  Midjourney8,

  // Image edit models

  #[serde(rename = "flux_pro_kontext_max")]
  FluxProKontextMax,
  #[serde(rename = "qwen_edit_2511_angles")]
  QwenEdit2511Angles,
  #[serde(rename = "flux_2_lora_angles")]
  Flux2LoraAngles,

  // Inpainting models

  #[serde(rename = "flux_dev_juggernaut")]
  FluxDevJuggernaut,
  #[serde(rename = "flux_pro_1")]
  FluxPro1,

  // Local models, run on the user's own ComfyUI server (GenerationProvider::ComfyUi).

  #[serde(rename = "comfyui_flux_1_schnell")]
  ComfyUiFlux1Schnell,
  #[serde(rename = "comfyui_flux_1_dev")]
  ComfyUiFlux1Dev,
  #[serde(rename = "comfyui_sdxl")]
  ComfyUiSdxl,
  #[serde(rename = "comfyui_flux_kontext")]
  ComfyUiFluxKontext,
}

/// How a local model runs on ComfyUI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComfyUiWorkflow {
  /// An all-in-one checkpoint: text to image, or image to image from one start image.
  Checkpoint(ModelFamily),
  /// FLUX.1 Kontext [dev]: instruction edits and fusion of several reference images.
  Kontext,
}

impl TauriImageModel {
  /// The ComfyUI workflow for this model, if it is a local model.
  pub fn comfyui_workflow(self) -> Option<ComfyUiWorkflow> {
    match self {
      Self::ComfyUiFlux1Schnell => Some(ComfyUiWorkflow::Checkpoint(ModelFamily::FluxSchnell)),
      Self::ComfyUiFlux1Dev => Some(ComfyUiWorkflow::Checkpoint(ModelFamily::FluxDev)),
      Self::ComfyUiSdxl => Some(ComfyUiWorkflow::Checkpoint(ModelFamily::Sdxl)),
      Self::ComfyUiFluxKontext => Some(ComfyUiWorkflow::Kontext),
      _ => None,
    }
  }
}
