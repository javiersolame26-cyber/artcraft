use std::time::Duration;

use log::info;
use reqwest::multipart::{Form, Part};
use reqwest::{Response, StatusCode};
use serde_json::Value;
use url::Url;

use crate::api::checkpoints::parse_checkpoint_names;
use crate::api::history::{parse_history, OutputImage, PromptHistory};
use crate::api::queue::{parse_queue, QueueSnapshot};
use crate::api::queue_prompt::{parse_queued_prompt, parse_rejection, QueuePromptRequest, QueuedPrompt};
use crate::api::system_stats::{parse_system_stats, SystemStats};
use crate::api::upload_image::{parse_uploaded_image, UploadedImage};
use crate::error::ComfyUiError;

pub const DEFAULT_COMFYUI_URL: &str = "http://127.0.0.1:8188";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);
const HISTORY_PATH_SEGMENT: &str = "history/";

/// Talks to one ComfyUI server.
///
/// ComfyUI normally runs on the user's own machine, so the client never goes
/// through a system proxy (corporate proxies tend to break loopback traffic).
#[derive(Clone, Debug)]
pub struct ComfyUiClient {
  base_url: Url,
  http: reqwest::Client,
}

impl ComfyUiClient {
  /// Accepts `http://host:port`, `host:port` or a URL with a path prefix
  /// (e.g. behind a reverse proxy).
  pub fn new(address: &str) -> Result<Self, ComfyUiError> {
    let base_url = normalize_base_url(address)?;
    let http = reqwest::Client::builder()
      .no_proxy()
      .connect_timeout(CONNECT_TIMEOUT)
      .build()?;
    Ok(Self { base_url, http })
  }

  /// Rebuild a client from a URL produced by [`Self::history_url`], returning
  /// it with the prompt id. Polling uses this so jobs keep working even if the
  /// configured address changes while they run.
  pub fn for_history_url(history_url: &str) -> Result<(Self, String), ComfyUiError> {
    let (base, prompt_id) = history_url
      .rsplit_once(HISTORY_PATH_SEGMENT)
      .filter(|(_, id)| !id.is_empty() && !id.contains('/'))
      .ok_or_else(|| ComfyUiError::InvalidBaseUrl(history_url.to_string()))?;
    Ok((Self::new(base)?, prompt_id.to_string()))
  }

  pub fn base_url(&self) -> &Url {
    &self.base_url
  }

  pub fn history_url(&self, prompt_id: &str) -> Url {
    self.endpoint(&format!("{}{}", HISTORY_PATH_SEGMENT, prompt_id))
  }

  pub fn view_url(&self, image: &OutputImage) -> Url {
    let mut url = self.endpoint("view");
    url.query_pairs_mut()
      .append_pair("filename", &image.filename)
      .append_pair("subfolder", &image.subfolder)
      .append_pair("type", &image.folder_type);
    url
  }

  pub async fn get_system_stats(&self) -> Result<SystemStats, ComfyUiError> {
    let body = self.get_text("system_stats").await?;
    parse_system_stats(&body)
  }

  pub async fn list_checkpoints(&self) -> Result<Vec<String>, ComfyUiError> {
    let body = self.get_text("object_info/CheckpointLoaderSimple").await?;
    parse_checkpoint_names(&body)
  }

  /// Store an input image on the server for a `LoadImage` node.
  pub async fn upload_image(&self, bytes: Vec<u8>, filename: &str) -> Result<UploadedImage, ComfyUiError> {
    info!("Uploading {} bytes to ComfyUI as {}", bytes.len(), filename);
    let part = Part::bytes(bytes)
      .file_name(filename.to_string())
      .mime_str(mime_for_filename(filename))?;
    let form = Form::new()
      .part("image", part)
      .text("type", "input")
      .text("overwrite", "true");
    let response = self.http.post(self.endpoint("upload/image"))
      .multipart(form)
      .timeout(TRANSFER_TIMEOUT)
      .send()
      .await
      .map_err(ComfyUiError::from_send_error)?;
    let body = ok_body(response).await?;
    parse_uploaded_image(&body)
  }

  /// Queue an API-format workflow. ComfyUI validates it before accepting,
  /// so a missing model file fails here rather than later.
  pub async fn queue_prompt(&self, workflow: &Value, client_id: &str) -> Result<QueuedPrompt, ComfyUiError> {
    let response = self.http.post(self.endpoint("prompt"))
      .json(&QueuePromptRequest { prompt: workflow, client_id })
      .timeout(REQUEST_TIMEOUT)
      .send()
      .await
      .map_err(ComfyUiError::from_send_error)?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
      return Err(parse_rejection(status.as_u16(), &body));
    }
    parse_queued_prompt(&body)
  }

  /// `None` until the prompt has finished (or if the server never saw it).
  pub async fn get_history(&self, prompt_id: &str) -> Result<Option<PromptHistory>, ComfyUiError> {
    let body = self.get_text(&format!("{}{}", HISTORY_PATH_SEGMENT, prompt_id)).await?;
    parse_history(&body, prompt_id)
  }

  pub async fn get_queue(&self) -> Result<QueueSnapshot, ComfyUiError> {
    let body = self.get_text("queue").await?;
    parse_queue(&body)
  }

  pub async fn download_image(&self, image: &OutputImage) -> Result<Vec<u8>, ComfyUiError> {
    let response = self.http.get(self.view_url(image))
      .timeout(TRANSFER_TIMEOUT)
      .send()
      .await
      .map_err(ComfyUiError::from_send_error)?;
    if response.status() != StatusCode::OK {
      let status = response.status().as_u16();
      let body = response.text().await.unwrap_or_default();
      return Err(ComfyUiError::BadStatus { status, body });
    }
    Ok(response.bytes().await?.to_vec())
  }

  /// Ask ComfyUI to stop whatever it is running right now.
  pub async fn interrupt(&self) -> Result<(), ComfyUiError> {
    let response = self.http.post(self.endpoint("interrupt"))
      .timeout(REQUEST_TIMEOUT)
      .send()
      .await
      .map_err(ComfyUiError::from_send_error)?;
    ok_body(response).await.map(|_| ())
  }

  async fn get_text(&self, path: &str) -> Result<String, ComfyUiError> {
    let response = self.http.get(self.endpoint(path))
      .timeout(REQUEST_TIMEOUT)
      .send()
      .await
      .map_err(ComfyUiError::from_send_error)?;
    ok_body(response).await
  }

  fn endpoint(&self, path: &str) -> Url {
    // The base always ends with '/', so joining a relative path cannot fail
    // or drop a reverse-proxy prefix.
    self.base_url.join(path).expect("relative endpoint path")
  }
}

async fn ok_body(response: Response) -> Result<String, ComfyUiError> {
  let status = response.status();
  let body = response.text().await?;
  if status.is_success() {
    Ok(body)
  } else {
    Err(ComfyUiError::BadStatus { status: status.as_u16(), body })
  }
}

fn normalize_base_url(address: &str) -> Result<Url, ComfyUiError> {
  let trimmed = address.trim();
  if trimmed.is_empty() {
    return Err(ComfyUiError::InvalidBaseUrl(address.to_string()));
  }
  let with_scheme = if trimmed.contains("://") { trimmed.to_string() } else { format!("http://{}", trimmed) };
  let with_slash = format!("{}/", with_scheme.trim_end_matches('/'));
  let url = Url::parse(&with_slash).map_err(|_| ComfyUiError::InvalidBaseUrl(address.to_string()))?;
  if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
    return Err(ComfyUiError::InvalidBaseUrl(address.to_string()));
  }
  Ok(url)
}

fn mime_for_filename(filename: &str) -> &'static str {
  let lower = filename.to_ascii_lowercase();
  if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
    "image/jpeg"
  } else if lower.ends_with(".webp") {
    "image/webp"
  } else {
    "image/png"
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;
  use crate::api::history::PromptStatus;
  use crate::test_server::{spawn_test_server, TestRoute};

  const PROMPT_ID: &str = "0c2f6d1a-1111-4222-8333-444455556666";

  mod address_tests {
    use super::*;

    #[test]
    fn adds_scheme_and_trailing_slash() {
      let client = ComfyUiClient::new("127.0.0.1:8188").unwrap();
      assert_eq!(client.base_url().as_str(), "http://127.0.0.1:8188/");
    }

    #[test]
    fn keeps_reverse_proxy_prefix() {
      let client = ComfyUiClient::new("https://studio.lan/comfy/").unwrap();
      assert_eq!(client.history_url(PROMPT_ID).as_str(), format!("https://studio.lan/comfy/history/{}", PROMPT_ID));
    }

    #[test]
    fn rejects_empty_and_non_http() {
      assert!(matches!(ComfyUiClient::new("  "), Err(ComfyUiError::InvalidBaseUrl(_))));
      assert!(matches!(ComfyUiClient::new("ftp://host"), Err(ComfyUiError::InvalidBaseUrl(_))));
    }

    #[test]
    fn round_trips_history_url() {
      let client = ComfyUiClient::new("http://192.168.1.20:8188").unwrap();
      let history_url = client.history_url(PROMPT_ID).to_string();
      let (rebuilt, prompt_id) = ComfyUiClient::for_history_url(&history_url).unwrap();
      assert_eq!(prompt_id, PROMPT_ID);
      assert_eq!(rebuilt.base_url(), client.base_url());
    }

    #[test]
    fn view_url_escapes_query() {
      let client = ComfyUiClient::new(DEFAULT_COMFYUI_URL).unwrap();
      let image = OutputImage { filename: "ArtCraft 00001_.png".into(), subfolder: "ArtCraft".into(), folder_type: "output".into() };
      assert_eq!(
        client.view_url(&image).as_str(),
        "http://127.0.0.1:8188/view?filename=ArtCraft+00001_.png&subfolder=ArtCraft&type=output",
      );
    }
  }

  mod server_tests {
    use super::*;

    #[tokio::test]
    async fn queue_prompt_sends_workflow_and_client_id() {
      let (base_url, captured) = spawn_test_server(vec![
        TestRoute::json("POST", "/prompt", 200, &format!(r#"{{"prompt_id": "{}", "number": 1, "node_errors": {{}}}}"#, PROMPT_ID)),
      ]).await;
      let client = ComfyUiClient::new(&base_url).unwrap();
      let workflow = json!({"1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "x.safetensors"}}});

      let queued = client.queue_prompt(&workflow, "artcraft-test").await.unwrap();

      assert_eq!(queued.prompt_id, PROMPT_ID);
      let request = captured.lock().unwrap()[0].clone();
      let body: Value = serde_json::from_slice(&request.body).unwrap();
      assert_eq!(body["client_id"], "artcraft-test");
      assert_eq!(body["prompt"], workflow);
    }

    #[tokio::test]
    async fn queue_prompt_reports_validation_errors() {
      let rejection = r#"{"error": {"message": "Prompt outputs failed validation", "details": ""},
        "node_errors": {"1": {"errors": [{"message": "Value not in list", "details": "ckpt_name: 'missing.safetensors' not in []"}], "class_type": "CheckpointLoaderSimple"}}}"#;
      let (base_url, _) = spawn_test_server(vec![TestRoute::json("POST", "/prompt", 400, rejection)]).await;
      let client = ComfyUiClient::new(&base_url).unwrap();

      let err = client.queue_prompt(&json!({}), "artcraft-test").await.unwrap_err();

      let ComfyUiError::PromptRejected { message, .. } = err else { panic!("expected rejection, got {:?}", err) };
      assert!(message.contains("missing.safetensors"));
    }

    #[tokio::test]
    async fn history_then_download() {
      let history = format!(r#"{{"{PROMPT_ID}": {{"outputs": {{"8": {{"images": [{{"filename": "ArtCraft_00001_.png", "subfolder": "ArtCraft", "type": "output"}}]}}}},
        "status": {{"status_str": "success", "completed": true, "messages": []}}}}}}"#);
      let (base_url, captured) = spawn_test_server(vec![
        TestRoute::json("GET", &format!("/history/{}", PROMPT_ID), 200, &history),
        TestRoute::bytes("GET", "/view?", 200, "image/png", b"\x89PNG fake".to_vec()),
      ]).await;
      let client = ComfyUiClient::new(&base_url).unwrap();

      let entry = client.get_history(PROMPT_ID).await.unwrap().unwrap();
      assert_eq!(entry.status, PromptStatus::Success);
      let bytes = client.download_image(&entry.output_images[0]).await.unwrap();

      assert_eq!(bytes, b"\x89PNG fake");
      let paths: Vec<String> = captured.lock().unwrap().iter().map(|r| r.path.clone()).collect();
      assert_eq!(paths[1], "/view?filename=ArtCraft_00001_.png&subfolder=ArtCraft&type=output");
    }

    #[tokio::test]
    async fn upload_image_sends_multipart() {
      let (base_url, captured) = spawn_test_server(vec![
        TestRoute::json("POST", "/upload/image", 200, r#"{"name": "scene.png", "subfolder": "", "type": "input"}"#),
      ]).await;
      let client = ComfyUiClient::new(&base_url).unwrap();

      let uploaded = client.upload_image(b"pixels".to_vec(), "scene.png").await.unwrap();

      assert_eq!(uploaded.load_image_value(), "scene.png");
      let request = captured.lock().unwrap()[0].clone();
      assert!(request.content_type.starts_with("multipart/form-data"));
      let body = String::from_utf8_lossy(&request.body);
      assert!(body.contains(r#"name="image"; filename="scene.png""#));
      assert!(body.contains("pixels"));
    }

    #[tokio::test]
    async fn stopped_server_is_unreachable() {
      // Bind and drop a listener to get a port nobody is serving.
      let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
      let address = listener.local_addr().unwrap().to_string();
      drop(listener);
      let client = ComfyUiClient::new(&address).unwrap();

      let err = client.get_system_stats().await.unwrap_err();

      assert!(err.is_unreachable(), "expected unreachable, got {:?}", err);
    }
  }
}
