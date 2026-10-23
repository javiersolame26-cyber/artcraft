use std::io::Cursor;

use artcraft_api_defs::prompts::create_prompt::CreatePromptRequest;
use artcraft_client::endpoints::prompts::create_prompt::create_prompt;
use artcraft_client::utils::api_host::ApiHost;
use comfyui_client::client::ComfyUiClient;
use comfyui_client::error::ComfyUiError;
use comfyui_client::workflows::dimensions::dimensions_for_aspect_ratio;
use comfyui_client::workflows::image_workflow::{build_image_workflow, ImageWorkflowParams, InitImage};
use comfyui_client::workflows::model_family::ModelFamily;
use enums::common::generation::common_aspect_ratio::CommonAspectRatio;
use enums::common::generation::common_generation_mode::CommonGenerationMode;
use enums::common::generation::common_model_type::CommonModelType;
use enums::common::generation_provider::GenerationProvider;
use enums::tauri::tasks::task_type::TaskType;
use image::ImageReader;
use log::{error, info, warn};
use tokens::tokens::media_files::MediaFileToken;
use tokens::tokens::prompts::PromptToken;

use crate::core::api_adapters::models::image::tauri_image_model_to_generation_model::tauri_image_model_to_generation_model;
use crate::core::commands::enqueue::generate_error::{BadInputReason, GenerateError};
use crate::core::commands::enqueue::task_enqueue_success::TaskEnqueueSuccess;
use crate::core::commands::generate::generate_image::providers::artcraft_router::utils::map_media_files_to_urls::map_media_file_tokens_to_cdn_urls;
use crate::core::commands::generate::generate_image::tauri_generate_image_request::TauriGenerateImageRequest;
use crate::core::state::app_env_configs::app_env_configs::AppEnvConfigs;
use crate::core::state::comfyui_settings::{ComfyUiSettings, ComfyUiSettingsStore};
use crate::services::storyteller::state::storyteller_credential_manager::StorytellerCredentialManager;

/// Identifies ArtCraft's prompts in ComfyUI's queue.
const COMFYUI_CLIENT_ID: &str = "artcraft-desktop";

/// Outputs land in `ComfyUI/output/ArtCraft/`.
const FILENAME_PREFIX: &str = "ArtCraft/artcraft";

/// One pass renders the whole batch, so keep it within consumer-GPU memory.
const MAX_BATCH_SIZE: u32 = 4;

/// Generate an image on the user's own ComfyUI server.
///
/// The prompt is queued right away; `poll_comfyui_tasks` picks up the result
/// and files it in the library like any other provider's output.
pub async fn handle_comfyui(
  request: &TauriGenerateImageRequest,
  settings_store: &ComfyUiSettingsStore,
  app_env_configs: &AppEnvConfigs,
  storyteller_creds_manager: &StorytellerCredentialManager,
) -> Result<TaskEnqueueSuccess, GenerateError> {
  let tauri_model = request.model.ok_or(GenerateError::no_model_specified())?;
  let family = tauri_model.comfyui_family().ok_or_else(|| GenerateError::BadInput(
    BadInputReason::WrongImageArguments(format!("Model {:?} can't run on ComfyUI. Pick one of the local models.", tauri_model)),
  ))?;

  let settings = settings_store.get();
  let client = ComfyUiClient::new(&settings.server_url).map_err(|err| comfyui_error(&settings, err))?;

  let prompt = request.prompt.as_deref().map(str::trim).unwrap_or_default();
  let maybe_init_bytes = load_init_image(request, &app_env_configs.storyteller_host).await?;

  if prompt.is_empty() && maybe_init_bytes.is_none() {
    return Err(GenerateError::BadInput(BadInputReason::WrongImageArguments("Describe the image you want to create.".to_string())));
  }

  let (width, height) = plan_dimensions(request.aspect_ratio, maybe_init_bytes.as_deref());
  let batch_size = plan_batch_size(request.batch_size)?;

  let maybe_uploaded = match maybe_init_bytes {
    Some(bytes) => {
      let filename = format!("artcraft_input_{}.png", uuid_utils::uuid::generate_random_uuid());
      Some(client.upload_image(bytes, &filename).await.map_err(|err| comfyui_error(&settings, err))?)
    }
    None => None,
  };
  let maybe_load_image_value = maybe_uploaded.as_ref().map(|image| image.load_image_value());

  let workflow = build_image_workflow(&ImageWorkflowParams {
    family,
    checkpoint: settings.checkpoint_for(family),
    prompt,
    maybe_negative_prompt: None,
    width,
    height,
    batch_size,
    seed: random_seed(),
    maybe_init_image: maybe_load_image_value.as_deref().map(|value| InitImage {
      load_image_value: value,
      denoise: settings.image_edit_strength,
    }),
    filename_prefix: FILENAME_PREFIX,
  });

  info!(
    "Queueing ComfyUI {:?} workflow on {}: {}x{}, batch {}, image-to-image: {}",
    family, client.base_url(), width, height, batch_size, maybe_load_image_value.is_some(),
  );

  let queued = client.queue_prompt(&workflow, COMFYUI_CLIENT_ID).await
    .map_err(|err| comfyui_error(&settings, err))?;

  info!("ComfyUI accepted prompt {}", queued.prompt_id);

  let maybe_prompt_token = create_prompt_record(
    request,
    family,
    prompt,
    batch_size,
    maybe_load_image_value.is_some(),
    &app_env_configs.storyteller_host,
    storyteller_creds_manager,
  ).await;

  Ok(TaskEnqueueSuccess {
    task_type: TaskType::ImageGeneration,
    model: Some(tauri_image_model_to_generation_model(tauri_model)),
    provider: GenerationProvider::ComfyUi,
    provider_job_id: Some(queued.prompt_id.clone()),
    maybe_queue_status_url: Some(client.history_url(&queued.prompt_id).to_string()),
    maybe_queue_response_url: None,
    maybe_prompt_token,
  })
}

// ── Input image ──

/// The image to start from, in the same precedence the other providers use:
/// canvas, then 3D scene, then the first reference image.
async fn load_init_image(
  request: &TauriGenerateImageRequest,
  api_host: &ApiHost,
) -> Result<Option<Vec<u8>>, GenerateError> {
  let raw_or_token = [
    (request.canvas_image_raw_bytes.as_ref(), request.canvas_image_media_token.as_ref()),
    (request.scene_image_raw_bytes.as_ref(), request.scene_image_media_token.as_ref()),
    (None, request.image_media_tokens.as_ref().and_then(|tokens| tokens.first())),
  ];

  for (maybe_bytes, maybe_token) in raw_or_token {
    if let Some(bytes) = maybe_bytes.filter(|bytes| !bytes.is_empty()) {
      return Ok(Some(bytes.clone()));
    }
    if let Some(token) = maybe_token {
      return download_media_file(token, api_host).await.map(Some);
    }
  }

  Ok(None)
}

async fn download_media_file(token: &MediaFileToken, api_host: &ApiHost) -> Result<Vec<u8>, GenerateError> {
  let urls = map_media_file_tokens_to_cdn_urls(std::slice::from_ref(token), api_host).await?;
  let url = urls.into_iter().next().ok_or(GenerateError::BadInput(BadInputReason::RequiredSourceImageNotProvided))?;
  info!("Downloading input image {} for ComfyUI", url);
  let response = reqwest::get(&url).await
    .and_then(|response| response.error_for_status())
    .map_err(|err| GenerateError::AnyhowError(anyhow::anyhow!("Could not download input image {}: {:?}", url, err)))?;
  let bytes = response.bytes().await
    .map_err(|err| GenerateError::AnyhowError(anyhow::anyhow!("Could not read input image {}: {:?}", url, err)))?;
  Ok(bytes.to_vec())
}

// ── Planning ──

/// "Auto" follows the input image's shape when there is one.
fn plan_dimensions(maybe_aspect_ratio: Option<CommonAspectRatio>, maybe_init_image: Option<&[u8]>) -> (u32, u32) {
  if let Some((w, h)) = maybe_aspect_ratio.and_then(aspect_ratio_parts) {
    return dimensions_for_aspect_ratio(w, h);
  }
  match maybe_init_image.and_then(image_size) {
    Some((w, h)) => dimensions_for_aspect_ratio(w, h),
    None => dimensions_for_aspect_ratio(1, 1),
  }
}

fn aspect_ratio_parts(aspect_ratio: CommonAspectRatio) -> Option<(u32, u32)> {
  match aspect_ratio {
    CommonAspectRatio::Square | CommonAspectRatio::SquareHd => Some((1, 1)),
    CommonAspectRatio::WideThreeByTwo | CommonAspectRatio::Wide => Some((3, 2)),
    CommonAspectRatio::WideFourByThree => Some((4, 3)),
    CommonAspectRatio::WideFiveByFour => Some((5, 4)),
    CommonAspectRatio::WideSixteenByNine => Some((16, 9)),
    CommonAspectRatio::WideTwentyOneByNine => Some((21, 9)),
    CommonAspectRatio::TallTwoByThree | CommonAspectRatio::Tall => Some((2, 3)),
    CommonAspectRatio::TallThreeByFour => Some((3, 4)),
    CommonAspectRatio::TallFourByFive => Some((4, 5)),
    CommonAspectRatio::TallNineBySixteen => Some((9, 16)),
    CommonAspectRatio::TallNineByTwentyOne => Some((9, 21)),
    CommonAspectRatio::Auto | CommonAspectRatio::Auto2k | CommonAspectRatio::Auto3k | CommonAspectRatio::Auto4k => None,
  }
}

fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
  ImageReader::new(Cursor::new(bytes))
    .with_guessed_format()
    .ok()?
    .into_dimensions()
    .ok()
}

fn plan_batch_size(maybe_requested: Option<u32>) -> Result<u32, GenerateError> {
  match maybe_requested.unwrap_or(1) {
    0 => Err(GenerateError::BadInput(BadInputReason::InvalidNumberOfRequestedImages { requested: 0, min: 1, max: MAX_BATCH_SIZE })),
    n => Ok(n.min(MAX_BATCH_SIZE)),
  }
}

fn random_seed() -> u64 {
  // ComfyUI accepts any u64; stay within JavaScript's safe integer range so
  // the seed survives a trip through the frontend.
  rand::random::<u64>() & ((1 << 53) - 1)
}

// ── Library prompt record ──

/// Attach the prompt to the results in the ArtCraft library. Fails open.
async fn create_prompt_record(
  request: &TauriGenerateImageRequest,
  family: ModelFamily,
  prompt: &str,
  batch_size: u32,
  is_image_to_image: bool,
  api_host: &ApiHost,
  storyteller_creds_manager: &StorytellerCredentialManager,
) -> Option<PromptToken> {
  let creds = match storyteller_creds_manager.get_credentials() {
    Ok(Some(creds)) => creds,
    _ => {
      warn!("[ComfyUi] Not logged into ArtCraft, skipping prompt record");
      return None;
    }
  };

  let prompt_request = CreatePromptRequest {
    uuid_idempotency_token: uuid_utils::uuid::generate_random_uuid(),
    positive_prompt: Some(prompt.to_string()).filter(|p| !p.is_empty()),
    negative_prompt: None,
    model_type: match family {
      ModelFamily::FluxSchnell => Some(CommonModelType::Flux1Schnell),
      ModelFamily::FluxDev => Some(CommonModelType::Flux1Dev),
      ModelFamily::Sdxl => None,
    },
    // The ArtCraft backend doesn't know local providers; leave it unset.
    generation_provider: None,
    maybe_generation_mode: Some(if is_image_to_image { CommonGenerationMode::Edit } else { CommonGenerationMode::Text }),
    maybe_aspect_ratio: request.aspect_ratio,
    maybe_resolution: None,
    maybe_batch_count: Some(batch_size.min(255) as u8),
    maybe_generate_audio: None,
    maybe_duration_seconds: None,
  };

  match create_prompt(api_host, Some(&creds), prompt_request).await {
    Ok(response) => Some(response.prompt_token),
    Err(err) => {
      error!("[ComfyUi] Failed to create prompt record (continuing anyway): {:?}", err);
      None
    }
  }
}

// ── Errors ──

fn comfyui_error(settings: &ComfyUiSettings, err: ComfyUiError) -> GenerateError {
  warn!("[ComfyUi] Generation request failed: {:?}", err);
  let message = match &err {
    ComfyUiError::InvalidBaseUrl(_) => format!(
      "\"{}\" is not a valid ComfyUI address. Fix it in Settings → ComfyUI.", settings.server_url,
    ),
    err if err.is_unreachable() => format!(
      "Could not reach ComfyUI at {}. Start ComfyUI, then try again (the address is in Settings → ComfyUI).", settings.server_url,
    ),
    ComfyUiError::PromptRejected { message, .. } if message.contains("ckpt_name") => format!(
      "ComfyUI doesn't have the model file this needs. Put it in ComfyUI/models/checkpoints or pick another file in Settings → ComfyUI. Details: {}", message,
    ),
    ComfyUiError::PromptRejected { message, .. } => format!("ComfyUI rejected the request: {}", message),
    other => format!("ComfyUI error: {}", other),
  };
  GenerateError::ComfyUi(message)
}

#[cfg(test)]
mod tests {
  use super::*;

  mod dimension_tests {
    use super::*;

    #[test]
    fn explicit_aspect_ratio_wins() {
      assert_eq!(plan_dimensions(Some(CommonAspectRatio::WideSixteenByNine), None), (1344, 768));
      assert_eq!(plan_dimensions(Some(CommonAspectRatio::TallTwoByThree), None), (832, 1216));
    }

    #[test]
    fn auto_follows_input_image() {
      let png = encode_png(1600, 900);
      assert_eq!(plan_dimensions(Some(CommonAspectRatio::Auto), Some(&png)), (1344, 768));
      assert_eq!(plan_dimensions(None, Some(&png)), (1344, 768));
    }

    #[test]
    fn auto_without_image_is_square() {
      assert_eq!(plan_dimensions(None, None), (1024, 1024));
      assert_eq!(plan_dimensions(Some(CommonAspectRatio::Auto2k), Some(b"not an image")), (1024, 1024));
    }
  }

  mod batch_tests {
    use super::*;

    #[test]
    fn defaults_to_one_and_caps() {
      assert_eq!(plan_batch_size(None).unwrap(), 1);
      assert_eq!(plan_batch_size(Some(9)).unwrap(), MAX_BATCH_SIZE);
    }

    #[test]
    fn zero_is_rejected() {
      assert!(matches!(plan_batch_size(Some(0)), Err(GenerateError::BadInput(_))));
    }
  }

  #[test]
  fn seed_fits_javascript_numbers() {
    for _ in 0..100 {
      assert!(random_seed() < (1 << 53));
    }
  }

  mod error_tests {
    use super::*;

    #[test]
    fn missing_checkpoint_explains_where_files_go() {
      let err = comfyui_error(&ComfyUiSettings::default(), ComfyUiError::PromptRejected {
        message: "Prompt outputs failed validation (CheckpointLoaderSimple: Value not in list - ckpt_name: 'x' not in [])".to_string(),
        node_errors: String::new(),
      });
      let GenerateError::ComfyUi(message) = err else { panic!("expected ComfyUi error") };
      assert!(message.contains("models/checkpoints"));
    }

    #[test]
    fn invalid_address_is_explained() {
      let err = comfyui_error(&ComfyUiSettings::default(), ComfyUiError::InvalidBaseUrl("ftp://x".to_string()));
      let GenerateError::ComfyUi(message) = err else { panic!("expected ComfyUi error") };
      assert!(message.contains("Settings → ComfyUI"));
    }
  }

  /// The whole path a frontend request takes: Omni request JSON → native
  /// request → ComfyUI handler → a (fake) ComfyUI server.
  mod end_to_end_tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::core::commands::generate::omni::dispatch::decode_native;
    use crate::core::commands::generate::omni::request::OmniRequest;
    use crate::core::state::data_dir::app_data_root::AppDataRoot;

    const PROMPT_ID: &str = "e2e-prompt-1";
    const CHECKPOINT: &str = "my-flux-schnell.safetensors";

    #[tokio::test]
    async fn scene_image_request_queues_an_image_to_image_workflow() {
      let (base_url, captured) = fake_comfyui().await;
      let request = frontend_request(json!({
        "provider": "comfy_ui",
        "model": "comfyui_flux_1_schnell",
        "prompt": "a red fox in a snowy forest, cinematic",
        "aspect_ratio": "wide_sixteen_by_nine",
        "batch_size": 2,
        "scene_image_raw_bytes": encode_png(640, 360),
      }));
      let settings = ComfyUiSettingsStore::new(ComfyUiSettings {
        server_url: base_url.clone(),
        flux_schnell_checkpoint: CHECKPOINT.to_string(),
        image_edit_strength: 0.4,
        ..ComfyUiSettings::default()
      });
      let (configs, creds) = app_state();

      let success = handle_comfyui(&request, &settings, &configs, &creds).await.expect("enqueue should succeed");

      assert_eq!(success.provider, GenerationProvider::ComfyUi);
      assert_eq!(success.provider_job_id.as_deref(), Some(PROMPT_ID));
      assert_eq!(success.maybe_queue_status_url, Some(format!("{}/history/{}", base_url, PROMPT_ID)));

      let requests = captured.lock().unwrap().clone();
      assert_eq!(requests.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(), vec!["/upload/image", "/prompt"]);
      let body: Value = serde_json::from_slice(&requests[1].1).unwrap();
      let workflow = &body["prompt"];
      assert_eq!(body["client_id"], COMFYUI_CLIENT_ID);
      assert_eq!(workflow["1"]["inputs"]["ckpt_name"], CHECKPOINT);
      assert_eq!(workflow["2"]["inputs"]["text"], "a red fox in a snowy forest, cinematic");
      assert_eq!(workflow["6"]["inputs"]["image"], "uploaded_scene.png");
      assert_eq!(workflow["7"]["inputs"]["width"], 1344);
      assert_eq!(workflow["7"]["inputs"]["height"], 768);
      assert_eq!(workflow["9"]["inputs"]["amount"], 2);
      assert!(workflow.get("5").is_none(), "image-to-image must not start from an empty latent");
      let denoise = workflow["10"]["inputs"]["denoise"].as_f64().unwrap();
      assert!((denoise - 0.4).abs() < 1e-6);
    }

    #[tokio::test]
    async fn stopped_comfyui_gives_a_helpful_error() {
      let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
      let address = format!("http://{}", listener.local_addr().unwrap());
      drop(listener);
      let request = frontend_request(json!({ "provider": "comfy_ui", "model": "comfyui_sdxl", "prompt": "a lighthouse" }));
      let settings = ComfyUiSettingsStore::new(ComfyUiSettings { server_url: address, ..ComfyUiSettings::default() });
      let (configs, creds) = app_state();

      let result = handle_comfyui(&request, &settings, &configs, &creds).await;

      let Err(GenerateError::ComfyUi(message)) = result else { panic!("expected a ComfyUI error") };
      assert!(message.starts_with("Could not reach ComfyUI"), "{}", message);
    }

    #[test]
    fn local_requests_skip_the_artcraft_omni_endpoint() {
      let omni: OmniRequest = serde_json::from_value(json!({ "provider": "comfy_ui", "model": "comfyui_flux_1_dev" })).unwrap();
      assert!(!omni.uses_artcraft());
    }

    fn frontend_request(value: Value) -> TauriGenerateImageRequest {
      let omni: OmniRequest = serde_json::from_value(value).unwrap();
      decode_native(omni).unwrap_or_else(|err| panic!("decode failed: {:?}", err.error_message))
    }

    fn app_state() -> (AppEnvConfigs, StorytellerCredentialManager) {
      let dir = tempfile::tempdir().unwrap().keep();
      let root = AppDataRoot::create_existing(&dir).unwrap();
      (AppEnvConfigs { storyteller_host: ApiHost::Storyteller }, StorytellerCredentialManager::initialize_empty(&root))
    }

    /// Answers `/upload/image` and `/prompt` like ComfyUI and records each request.
    async fn fake_comfyui() -> (String, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
      let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
      let base_url = format!("http://{}", listener.local_addr().unwrap());
      let captured = Arc::new(Mutex::new(Vec::new()));
      let captured_for_server = captured.clone();
      tokio::spawn(async move {
        loop {
          let Ok((mut socket, _)) = listener.accept().await else { return };
          let (path, body) = read_request(&mut socket).await;
          let response = if path.starts_with("/upload/image") {
            r#"{"name": "uploaded_scene.png", "subfolder": "", "type": "input"}"#.to_string()
          } else {
            format!(r#"{{"prompt_id": "{}", "number": 1, "node_errors": {{}}}}"#, PROMPT_ID)
          };
          captured_for_server.lock().unwrap().push((path, body));
          let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len());
          let _ = socket.write_all(head.as_bytes()).await;
          let _ = socket.write_all(response.as_bytes()).await;
        }
      });
      (base_url, captured)
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
      let mut buffer = Vec::new();
      let header_end = loop {
        let mut chunk = [0u8; 8192];
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0, "connection closed before headers finished");
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
          break pos + 4;
        }
      };
      let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
      let path = head.split(' ').nth(1).unwrap_or_default().to_string();
      let content_length: usize = head.lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string()))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
      let mut body = buffer[header_end..].to_vec();
      while body.len() < content_length {
        let mut chunk = [0u8; 8192];
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 { break; }
        body.extend_from_slice(&chunk[..n]);
      }
      (path, body)
    }
  }

  fn encode_png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::new(width, height);
    let mut bytes = Vec::new();
    image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
  }
}
