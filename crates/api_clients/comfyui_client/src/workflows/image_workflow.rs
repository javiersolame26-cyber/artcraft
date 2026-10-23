use serde_json::{json, Map, Value};

use crate::workflows::model_family::ModelFamily;

// Node ids in the generated graph. Stable ids keep the history output
// order predictable and make the workflow easy to inspect in ComfyUI.
const CHECKPOINT: &str = "1";
const POSITIVE: &str = "2";
const NEGATIVE: &str = "3";
const GUIDANCE: &str = "4";
const EMPTY_LATENT: &str = "5";
const LOAD_IMAGE: &str = "6";
const SCALE_IMAGE: &str = "7";
const ENCODE_IMAGE: &str = "8";
const REPEAT_LATENT: &str = "9";
const SAMPLER: &str = "10";
const DECODE: &str = "11";
const SAVE: &str = "12";

/// Smallest denoise we send for image-to-image; below this the output is
/// indistinguishable from the input.
const MIN_DENOISE: f32 = 0.05;

/// Parameters for a single checkpoint text-to-image or image-to-image run.
#[derive(Clone, Debug)]
pub struct ImageWorkflowParams<'a> {
  pub family: ModelFamily,

  /// File name in ComfyUI's `models/checkpoints` folder.
  pub checkpoint: &'a str,

  pub prompt: &'a str,

  /// `None` uses the family default.
  pub maybe_negative_prompt: Option<&'a str>,

  pub width: u32,
  pub height: u32,

  /// Images generated in one pass.
  pub batch_size: u32,

  pub seed: u64,

  /// When present, the run starts from this image instead of noise.
  pub maybe_init_image: Option<InitImage<'a>>,

  /// SaveImage prefix; may include a subfolder (`ArtCraft/image`).
  pub filename_prefix: &'a str,
}

/// An image already uploaded to ComfyUI that seeds an image-to-image run.
#[derive(Clone, Debug)]
pub struct InitImage<'a> {
  /// The `LoadImage` value from [`crate::api::upload_image::UploadedImage::load_image_value`].
  pub load_image_value: &'a str,

  /// How much to change it: 0.0 keeps the input, 1.0 ignores it.
  pub denoise: f32,
}

/// Build an API-format workflow:
/// checkpoint → prompts → (FLUX guidance) → empty or encoded latent → KSampler → decode → save.
pub fn build_image_workflow(params: &ImageWorkflowParams<'_>) -> Value {
  let settings = params.family.sampler_settings();
  let negative = params.maybe_negative_prompt.unwrap_or(params.family.default_negative_prompt());
  let batch_size = params.batch_size.max(1);

  let mut nodes = Map::new();
  add(&mut nodes, CHECKPOINT, "CheckpointLoaderSimple", json!({ "ckpt_name": params.checkpoint }));
  add(&mut nodes, POSITIVE, "CLIPTextEncode", json!({ "text": params.prompt, "clip": [CHECKPOINT, 1] }));
  add(&mut nodes, NEGATIVE, "CLIPTextEncode", json!({ "text": negative, "clip": [CHECKPOINT, 1] }));

  let positive = match settings.maybe_flux_guidance {
    Some(guidance) => {
      add(&mut nodes, GUIDANCE, "FluxGuidance", json!({ "conditioning": [POSITIVE, 0], "guidance": guidance }));
      json!([GUIDANCE, 0])
    }
    None => json!([POSITIVE, 0]),
  };

  let (latent, denoise) = match &params.maybe_init_image {
    None => {
      add(&mut nodes, EMPTY_LATENT, params.family.empty_latent_node(), json!({
        "width": params.width,
        "height": params.height,
        "batch_size": batch_size,
      }));
      (json!([EMPTY_LATENT, 0]), 1.0)
    }
    Some(init) => (add_init_image_latent(&mut nodes, params, init, batch_size), init.denoise.clamp(MIN_DENOISE, 1.0)),
  };

  add(&mut nodes, SAMPLER, "KSampler", json!({
    "model": [CHECKPOINT, 0],
    "seed": params.seed,
    "steps": settings.steps,
    "cfg": settings.cfg,
    "sampler_name": settings.sampler_name,
    "scheduler": settings.scheduler,
    "positive": positive,
    "negative": [NEGATIVE, 0],
    "latent_image": latent,
    "denoise": denoise,
  }));
  add(&mut nodes, DECODE, "VAEDecode", json!({ "samples": [SAMPLER, 0], "vae": [CHECKPOINT, 2] }));
  add(&mut nodes, SAVE, "SaveImage", json!({ "images": [DECODE, 0], "filename_prefix": params.filename_prefix }));

  Value::Object(nodes)
}

/// Load the uploaded image, resize/crop it to the target size, encode it,
/// and repeat it when more than one image is requested.
fn add_init_image_latent(nodes: &mut Map<String, Value>, params: &ImageWorkflowParams<'_>, init: &InitImage<'_>, batch_size: u32) -> Value {
  add(nodes, LOAD_IMAGE, "LoadImage", json!({ "image": init.load_image_value }));
  add(nodes, SCALE_IMAGE, "ImageScale", json!({
    "image": [LOAD_IMAGE, 0],
    "upscale_method": "lanczos",
    "width": params.width,
    "height": params.height,
    "crop": "center",
  }));
  add(nodes, ENCODE_IMAGE, "VAEEncode", json!({ "pixels": [SCALE_IMAGE, 0], "vae": [CHECKPOINT, 2] }));
  if batch_size > 1 {
    add(nodes, REPEAT_LATENT, "RepeatLatentBatch", json!({ "samples": [ENCODE_IMAGE, 0], "amount": batch_size }));
    json!([REPEAT_LATENT, 0])
  } else {
    json!([ENCODE_IMAGE, 0])
  }
}

fn add(nodes: &mut Map<String, Value>, id: &str, class_type: &str, inputs: Value) {
  nodes.insert(id.to_string(), json!({ "class_type": class_type, "inputs": inputs }));
}

#[cfg(test)]
mod tests {
  use super::*;

  const CHECKPOINT_FILE: &str = "flux1-schnell-fp8.safetensors";
  const PROMPT: &str = "a lighthouse on a cliff during a storm";

  mod text_to_image_tests {
    use super::*;

    #[test]
    fn flux_schnell_graph() {
      let workflow = build_image_workflow(&params(ModelFamily::FluxSchnell));

      assert_eq!(class_types(&workflow), vec![
        ("1", "CheckpointLoaderSimple"), ("10", "KSampler"), ("11", "VAEDecode"), ("12", "SaveImage"),
        ("2", "CLIPTextEncode"), ("3", "CLIPTextEncode"), ("5", "EmptySD3LatentImage"),
      ]);
      let sampler = &workflow[SAMPLER]["inputs"];
      assert_eq!(sampler["steps"], 4);
      assert_eq!(sampler["cfg"], 1.0);
      assert_eq!(sampler["positive"], json!(["2", 0]));
      assert_eq!(sampler["latent_image"], json!(["5", 0]));
      assert_eq!(sampler["denoise"], 1.0);
      assert_eq!(sampler["seed"], 42);
      assert_eq!(workflow[POSITIVE]["inputs"]["text"], PROMPT);
      assert_eq!(workflow[NEGATIVE]["inputs"]["text"], "");
      assert_eq!(workflow[EMPTY_LATENT]["inputs"], json!({ "width": 1344, "height": 768, "batch_size": 2 }));
      assert_eq!(workflow[CHECKPOINT]["inputs"]["ckpt_name"], CHECKPOINT_FILE);
    }

    #[test]
    fn flux_dev_routes_positive_through_guidance() {
      let workflow = build_image_workflow(&params(ModelFamily::FluxDev));

      assert_eq!(workflow[GUIDANCE]["class_type"], "FluxGuidance");
      assert_eq!(workflow[GUIDANCE]["inputs"]["guidance"], 3.5);
      assert_eq!(workflow[SAMPLER]["inputs"]["positive"], json!(["4", 0]));
      assert_eq!(workflow[SAMPLER]["inputs"]["steps"], 20);
    }

    #[test]
    fn sdxl_uses_classic_latent_and_default_negative() {
      let workflow = build_image_workflow(&params(ModelFamily::Sdxl));

      assert_eq!(workflow[EMPTY_LATENT]["class_type"], "EmptyLatentImage");
      assert_eq!(workflow[SAMPLER]["inputs"]["cfg"], 6.5);
      assert!(workflow[NEGATIVE]["inputs"]["text"].as_str().unwrap().contains("watermark"));
    }

    #[test]
    fn explicit_negative_prompt_wins() {
      let workflow = build_image_workflow(&ImageWorkflowParams {
        maybe_negative_prompt: Some("people"),
        ..params(ModelFamily::Sdxl)
      });
      assert_eq!(workflow[NEGATIVE]["inputs"]["text"], "people");
    }

    #[test]
    fn zero_batch_becomes_one() {
      let workflow = build_image_workflow(&ImageWorkflowParams { batch_size: 0, ..params(ModelFamily::FluxSchnell) });
      assert_eq!(workflow[EMPTY_LATENT]["inputs"]["batch_size"], 1);
    }
  }

  mod image_to_image_tests {
    use super::*;

    #[test]
    fn encodes_scaled_init_image() {
      let workflow = build_image_workflow(&ImageWorkflowParams {
        maybe_init_image: Some(InitImage { load_image_value: "artcraft/scene.png", denoise: 0.6 }),
        batch_size: 1,
        ..params(ModelFamily::FluxSchnell)
      });

      assert!(workflow.get(EMPTY_LATENT).is_none());
      assert_eq!(workflow[LOAD_IMAGE]["inputs"]["image"], "artcraft/scene.png");
      assert_eq!(workflow[SCALE_IMAGE]["inputs"]["width"], 1344);
      assert_eq!(workflow[SCALE_IMAGE]["inputs"]["crop"], "center");
      assert_eq!(workflow[ENCODE_IMAGE]["inputs"]["pixels"], json!(["7", 0]));
      assert_eq!(workflow[SAMPLER]["inputs"]["latent_image"], json!(["8", 0]));
      let denoise = workflow[SAMPLER]["inputs"]["denoise"].as_f64().unwrap();
      assert!((denoise - 0.6).abs() < 1e-6);
    }

    #[test]
    fn repeats_latent_for_batches() {
      let workflow = build_image_workflow(&ImageWorkflowParams {
        maybe_init_image: Some(InitImage { load_image_value: "scene.png", denoise: 0.7 }),
        batch_size: 3,
        ..params(ModelFamily::Sdxl)
      });

      assert_eq!(workflow[REPEAT_LATENT]["inputs"], json!({ "samples": ["8", 0], "amount": 3 }));
      assert_eq!(workflow[SAMPLER]["inputs"]["latent_image"], json!(["9", 0]));
    }

    #[test]
    fn denoise_is_clamped() {
      let workflow = build_image_workflow(&ImageWorkflowParams {
        maybe_init_image: Some(InitImage { load_image_value: "scene.png", denoise: 0.0 }),
        ..params(ModelFamily::Sdxl)
      });
      let denoise = workflow[SAMPLER]["inputs"]["denoise"].as_f64().unwrap();
      assert!((denoise - MIN_DENOISE as f64).abs() < 1e-6);
    }
  }

  fn params(family: ModelFamily) -> ImageWorkflowParams<'static> {
    ImageWorkflowParams {
      family,
      checkpoint: CHECKPOINT_FILE,
      prompt: PROMPT,
      maybe_negative_prompt: None,
      width: 1344,
      height: 768,
      batch_size: 2,
      seed: 42,
      maybe_init_image: None,
      filename_prefix: "ArtCraft/image",
    }
  }

  /// Sorted by node id so the check doesn't depend on serde_json's map ordering.
  fn class_types(workflow: &Value) -> Vec<(&str, &str)> {
    let mut nodes: Vec<(&str, &str)> = workflow.as_object().unwrap().iter()
      .map(|(id, node)| (id.as_str(), node["class_type"].as_str().unwrap()))
      .collect();
    nodes.sort();
    nodes
  }
}
