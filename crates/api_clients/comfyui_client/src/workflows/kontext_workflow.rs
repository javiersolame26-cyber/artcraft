use serde_json::{json, Map, Value};

// Node ids in the generated graph.
const DIFFUSION_MODEL: &str = "1";
const TEXT_ENCODERS: &str = "2";
const VAE: &str = "3";
const PROMPT: &str = "4";
const REFERENCE: &str = "5";
const GUIDANCE: &str = "6";
const NEGATIVE: &str = "7";
const EMPTY_LATENT: &str = "8";
const SAMPLER: &str = "9";
const DECODE: &str = "10";
const SAVE: &str = "11";
const SCALE_REFERENCE: &str = "12";
const ENCODE_REFERENCE: &str = "13";
/// Reference image `i` is loaded by node `LOAD_IMAGE_BASE + i`.
const LOAD_IMAGE_BASE: usize = 20;
/// Reference image `i` (from the second on) is stitched on by node `STITCH_BASE + i`.
const STITCH_BASE: usize = 30;

// Settings from ComfyUI's FLUX.1 Kontext [dev] template.
const STEPS: u32 = 20;
const CFG: f32 = 1.0;
const SAMPLER_NAME: &str = "euler";
const SCHEDULER: &str = "simple";
const GUIDANCE_SCALE: f32 = 2.5;

/// Size of the stitched reference sheet. Kontext works best around one megapixel.
const REFERENCE_MEGAPIXELS: f32 = 1.0;
/// FLUX latents are 1/8 the image size and packed in 2×2 patches.
const REFERENCE_RESOLUTION_STEP: u32 = 16;

/// Most references we stitch together; past this each one gets too small.
pub const MAX_KONTEXT_REFERENCE_IMAGES: usize = 4;

/// Parameters for an instruction-based edit or fusion with FLUX.1 Kontext [dev].
///
/// The reference images are placed side by side on one sheet that Kontext
/// reads as context, so the prompt can refer to all of them ("put the jacket
/// from the second image on the person in the first").
#[derive(Clone, Debug)]
pub struct KontextWorkflowParams<'a> {
  pub files: KontextModelFiles<'a>,

  /// What to make from the references, in natural language.
  pub prompt: &'a str,

  /// `LoadImage` values of the uploaded references, in the order the user
  /// gave them. At least one; extras past [`MAX_KONTEXT_REFERENCE_IMAGES`] are ignored.
  pub reference_images: &'a [String],

  pub width: u32,
  pub height: u32,

  /// Images generated in one pass.
  pub batch_size: u32,

  pub seed: u64,

  /// SaveImage prefix; may include a subfolder (`ArtCraft/image`).
  pub filename_prefix: &'a str,
}

/// Kontext ships as separate files rather than one checkpoint.
#[derive(Clone, Debug)]
pub struct KontextModelFiles<'a> {
  /// In `models/diffusion_models`.
  pub diffusion_model: &'a str,
  /// CLIP-L text encoder, in `models/text_encoders`.
  pub clip_l: &'a str,
  /// T5-XXL text encoder, in `models/text_encoders`.
  pub t5xxl: &'a str,
  /// FLUX autoencoder, in `models/vae`.
  pub vae: &'a str,
}

/// Build an API-format workflow:
/// references → stitched sheet → reference latent → prompt + guidance → KSampler → decode → save.
pub fn build_kontext_workflow(params: &KontextWorkflowParams<'_>) -> Value {
  let mut nodes = Map::new();
  add(&mut nodes, DIFFUSION_MODEL, "UNETLoader", json!({
    "unet_name": params.files.diffusion_model,
    "weight_dtype": "default",
  }));
  add(&mut nodes, TEXT_ENCODERS, "DualCLIPLoader", json!({
    "clip_name1": params.files.clip_l,
    "clip_name2": params.files.t5xxl,
    "type": "flux",
  }));
  add(&mut nodes, VAE, "VAELoader", json!({ "vae_name": params.files.vae }));
  add(&mut nodes, PROMPT, "CLIPTextEncode", json!({ "text": params.prompt, "clip": [TEXT_ENCODERS, 0] }));

  let positive = match add_reference_latent(&mut nodes, params.reference_images) {
    Some(latent) => {
      add(&mut nodes, REFERENCE, "ReferenceLatent", json!({ "conditioning": [PROMPT, 0], "latent": latent }));
      json!([REFERENCE, 0])
    }
    None => json!([PROMPT, 0]),
  };
  add(&mut nodes, GUIDANCE, "FluxGuidance", json!({ "conditioning": positive, "guidance": GUIDANCE_SCALE }));
  add(&mut nodes, NEGATIVE, "ConditioningZeroOut", json!({ "conditioning": [PROMPT, 0] }));
  add(&mut nodes, EMPTY_LATENT, "EmptySD3LatentImage", json!({
    "width": params.width,
    "height": params.height,
    "batch_size": params.batch_size.max(1),
  }));
  add(&mut nodes, SAMPLER, "KSampler", json!({
    "model": [DIFFUSION_MODEL, 0],
    "seed": params.seed,
    "steps": STEPS,
    "cfg": CFG,
    "sampler_name": SAMPLER_NAME,
    "scheduler": SCHEDULER,
    "positive": [GUIDANCE, 0],
    "negative": [NEGATIVE, 0],
    "latent_image": [EMPTY_LATENT, 0],
    "denoise": 1.0,
  }));
  add(&mut nodes, DECODE, "VAEDecode", json!({ "samples": [SAMPLER, 0], "vae": [VAE, 0] }));
  add(&mut nodes, SAVE, "SaveImage", json!({ "images": [DECODE, 0], "filename_prefix": params.filename_prefix }));

  Value::Object(nodes)
}

/// Load the references, stitch them left to right at a common height, scale the
/// sheet to about one megapixel and encode it. `None` without references.
fn add_reference_latent(nodes: &mut Map<String, Value>, reference_images: &[String]) -> Option<Value> {
  let references = &reference_images[..reference_images.len().min(MAX_KONTEXT_REFERENCE_IMAGES)];
  let mut sheet: Option<Value> = None;

  for (i, load_image_value) in references.iter().enumerate() {
    let load_id = (LOAD_IMAGE_BASE + i).to_string();
    add(nodes, &load_id, "LoadImage", json!({ "image": load_image_value }));
    sheet = Some(match sheet {
      None => json!([load_id, 0]),
      Some(left) => {
        let stitch_id = (STITCH_BASE + i).to_string();
        add(nodes, &stitch_id, "ImageStitch", json!({
          "image1": left,
          "image2": [load_id, 0],
          "direction": "right",
          "match_image_size": true,
          "spacing_width": 0,
          "spacing_color": "white",
        }));
        json!([stitch_id, 0])
      }
    });
  }

  let sheet = sheet?;
  add(nodes, SCALE_REFERENCE, "ImageScaleToTotalPixels", json!({
    "image": sheet,
    "upscale_method": "lanczos",
    "megapixels": REFERENCE_MEGAPIXELS,
    "resolution_steps": REFERENCE_RESOLUTION_STEP,
  }));
  add(nodes, ENCODE_REFERENCE, "VAEEncode", json!({ "pixels": [SCALE_REFERENCE, 0], "vae": [VAE, 0] }));
  Some(json!([ENCODE_REFERENCE, 0]))
}

fn add(nodes: &mut Map<String, Value>, id: &str, class_type: &str, inputs: Value) {
  nodes.insert(id.to_string(), json!({ "class_type": class_type, "inputs": inputs }));
}

#[cfg(test)]
mod tests {
  use super::*;

  const PROMPT_TEXT: &str = "put the cat from the first image on the sofa from the second image";

  #[test]
  fn single_reference_graph() {
    let references = images(1);
    let workflow = build_kontext_workflow(&params(&references));

    assert_eq!(class_types(&workflow), vec![
      ("1", "UNETLoader"), ("10", "VAEDecode"), ("11", "SaveImage"), ("12", "ImageScaleToTotalPixels"),
      ("13", "VAEEncode"), ("2", "DualCLIPLoader"), ("20", "LoadImage"), ("3", "VAELoader"),
      ("4", "CLIPTextEncode"), ("5", "ReferenceLatent"), ("6", "FluxGuidance"), ("7", "ConditioningZeroOut"),
      ("8", "EmptySD3LatentImage"), ("9", "KSampler"),
    ]);
    assert_eq!(workflow[SCALE_REFERENCE]["inputs"]["image"], json!(["20", 0]));
    assert_eq!(workflow[REFERENCE]["inputs"], json!({ "conditioning": ["4", 0], "latent": ["13", 0] }));
    assert_eq!(workflow[GUIDANCE]["inputs"], json!({ "conditioning": ["5", 0], "guidance": 2.5 }));
    assert_eq!(workflow[PROMPT]["inputs"]["text"], PROMPT_TEXT);
  }

  #[test]
  fn sampler_follows_the_kontext_template() {
    let references = images(1);
    let workflow = build_kontext_workflow(&params(&references));
    let sampler = &workflow[SAMPLER]["inputs"];

    assert_eq!(sampler["steps"], 20);
    assert_eq!(sampler["cfg"], 1.0);
    assert_eq!(sampler["sampler_name"], "euler");
    assert_eq!(sampler["scheduler"], "simple");
    assert_eq!(sampler["positive"], json!(["6", 0]));
    assert_eq!(sampler["negative"], json!(["7", 0]));
    assert_eq!(sampler["latent_image"], json!(["8", 0]));
    assert_eq!(sampler["seed"], 7);
    assert_eq!(workflow[EMPTY_LATENT]["inputs"], json!({ "width": 1024, "height": 1024, "batch_size": 1 }));
  }

  #[test]
  fn model_files_go_to_their_loaders() {
    let references = images(1);
    let workflow = build_kontext_workflow(&params(&references));

    assert_eq!(workflow[DIFFUSION_MODEL]["inputs"]["unet_name"], "flux1-dev-kontext_fp8_scaled.safetensors");
    assert_eq!(workflow[TEXT_ENCODERS]["inputs"], json!({
      "clip_name1": "clip_l.safetensors",
      "clip_name2": "t5xxl_fp8_e4m3fn_scaled.safetensors",
      "type": "flux",
    }));
    assert_eq!(workflow[VAE]["inputs"]["vae_name"], "ae.safetensors");
  }

  #[test]
  fn stitches_references_left_to_right() {
    let references = images(3);
    let workflow = build_kontext_workflow(&params(&references));

    assert_eq!(workflow["20"]["inputs"]["image"], "ref_0.png");
    assert_eq!(workflow["22"]["inputs"]["image"], "ref_2.png");
    assert_eq!(workflow["31"]["inputs"]["image1"], json!(["20", 0]));
    assert_eq!(workflow["31"]["inputs"]["image2"], json!(["21", 0]));
    assert_eq!(workflow["32"]["inputs"]["image1"], json!(["31", 0]));
    assert_eq!(workflow["32"]["inputs"]["image2"], json!(["22", 0]));
    assert_eq!(workflow["32"]["inputs"]["direction"], "right");
    assert_eq!(workflow[SCALE_REFERENCE]["inputs"]["image"], json!(["32", 0]));
  }

  #[test]
  fn extra_references_are_dropped() {
    let references = images(6);
    let workflow = build_kontext_workflow(&params(&references));

    assert!(workflow.get("23").is_some());
    assert!(workflow.get("24").is_none());
    assert_eq!(workflow[SCALE_REFERENCE]["inputs"]["image"], json!(["33", 0]));
  }

  #[test]
  fn without_references_it_is_plain_text_to_image() {
    let workflow = build_kontext_workflow(&params(&[]));

    assert!(workflow.get(REFERENCE).is_none());
    assert!(workflow.get(ENCODE_REFERENCE).is_none());
    assert_eq!(workflow[GUIDANCE]["inputs"]["conditioning"], json!(["4", 0]));
  }

  fn params(reference_images: &[String]) -> KontextWorkflowParams<'_> {
    KontextWorkflowParams {
      files: KontextModelFiles {
        diffusion_model: "flux1-dev-kontext_fp8_scaled.safetensors",
        clip_l: "clip_l.safetensors",
        t5xxl: "t5xxl_fp8_e4m3fn_scaled.safetensors",
        vae: "ae.safetensors",
      },
      prompt: PROMPT_TEXT,
      reference_images,
      width: 1024,
      height: 1024,
      batch_size: 0,
      seed: 7,
      filename_prefix: "ArtCraft/artcraft",
    }
  }

  fn images(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("ref_{}.png", i)).collect()
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
