/// The kind of checkpoint a workflow drives. Each family needs a different
/// latent type and sampler setup.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ModelFamily {
  /// FLUX.1 [schnell]: 4-step distilled model (Apache 2.0).
  FluxSchnell,
  /// FLUX.1 [dev]: guidance-distilled model, ~20 steps.
  FluxDev,
  /// Stable Diffusion XL and its fine-tunes.
  Sdxl,
}

/// Sampler configuration for one family.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SamplerSettings {
  pub steps: u32,
  pub cfg: f32,
  pub sampler_name: &'static str,
  pub scheduler: &'static str,
  /// FLUX dev reads its guidance from conditioning (`FluxGuidance`), not CFG.
  pub maybe_flux_guidance: Option<f32>,
}

const SDXL_DEFAULT_NEGATIVE_PROMPT: &str = "blurry, low quality, jpeg artifacts, watermark, text, signature";

impl ModelFamily {
  pub fn sampler_settings(self) -> SamplerSettings {
    match self {
      Self::FluxSchnell => SamplerSettings {
        steps: 4,
        cfg: 1.0,
        sampler_name: "euler",
        scheduler: "simple",
        maybe_flux_guidance: None,
      },
      Self::FluxDev => SamplerSettings {
        steps: 20,
        cfg: 1.0,
        sampler_name: "euler",
        scheduler: "simple",
        maybe_flux_guidance: Some(3.5),
      },
      Self::Sdxl => SamplerSettings {
        steps: 25,
        cfg: 6.5,
        sampler_name: "dpmpp_2m",
        scheduler: "karras",
        maybe_flux_guidance: None,
      },
    }
  }

  /// FLUX runs at CFG 1, which ignores the negative prompt entirely.
  pub fn default_negative_prompt(self) -> &'static str {
    match self {
      Self::FluxSchnell | Self::FluxDev => "",
      Self::Sdxl => SDXL_DEFAULT_NEGATIVE_PROMPT,
    }
  }

  /// FLUX uses 16-channel latents (same as SD3); SDXL uses 4-channel latents.
  pub fn empty_latent_node(self) -> &'static str {
    match self {
      Self::FluxSchnell | Self::FluxDev => "EmptySD3LatentImage",
      Self::Sdxl => "EmptyLatentImage",
    }
  }
}
