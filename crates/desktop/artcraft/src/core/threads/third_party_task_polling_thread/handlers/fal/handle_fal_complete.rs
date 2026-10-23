use crate::core::state::app_env_configs::app_env_configs::AppEnvConfigs;
use crate::core::state::data_dir::app_data_root::AppDataRoot;
use crate::core::state::data_dir::trait_data_subdir::DataSubdir;
use crate::core::state::task_database::TaskDatabase;
use crate::core::threads::third_party_task_polling_thread::handlers::common::finish_task_with_local_results::{
  finish_task_with_local_results, FinishTaskArgs,
};
use crate::core::utils::auto_download::auto_download_task_urls;
use crate::services::storyteller::state::storyteller_credential_manager::StorytellerCredentialManager;
use enums::common::generation_provider::GenerationProvider;
use enums::tauri::tasks::task_media_file_class::TaskMediaFileClass;
use fal_client::polling::poll_job_response::poll_job_response::PollJobResponse;
use fal_client::polling::poll_job_response::success_case_extractors::PollResponseExtractedContents;
use log::{error, info, warn};
use sqlite_tasks::queries::task::Task;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tauri::AppHandle;
use uuid_utils::uuid::generate_random_uuid;

pub async fn handle_fal_complete(
  app_handle: &AppHandle,
  app_env_configs: &AppEnvConfigs,
  app_data_root: &AppDataRoot,
  task_database: &TaskDatabase,
  storyteller_creds_manager: &StorytellerCredentialManager,
  task: &Task,
  job_response: PollJobResponse,
) {
  info!("[FalComplete] Handling completed task {}", task.id.as_str());

  let result = handle_fal_complete_inner(
    app_handle,
    app_env_configs,
    app_data_root,
    task_database,
    storyteller_creds_manager,
    task,
    job_response,
  ).await;

  if let Err(err) = result {
    error!("[FalComplete] Failed to handle task {}: {:?}", task.id.as_str(), err);
  }
}

async fn handle_fal_complete_inner(
  app_handle: &AppHandle,
  app_env_configs: &AppEnvConfigs,
  app_data_root: &AppDataRoot,
  task_database: &TaskDatabase,
  storyteller_creds_manager: &StorytellerCredentialManager,
  task: &Task,
  job_response: PollJobResponse,
) -> Result<(), Box<dyn std::error::Error>> {
  let creds = storyteller_creds_manager.get_credentials()?
    .ok_or("No Storyteller credentials available")?;

  let extracted = job_response.extracted_contents;

  // Determine what kind of media we got and collect download URLs.
  let (urls, media_class) = collect_media_urls(task, &extracted)?;

  if urls.is_empty() {
    warn!("[FalComplete] Task {} completed but no downloadable media found in response", task.id.as_str());
    //warn!("[FalComplete] Raw body: {}", &job_response.raw_body[..job_response.raw_body.len().min(500)]);
    return Ok(());
  }

  // Save local copies before uploading or announcing completion.
  let download_urls = urls.iter().map(|url| reqwest::Url::parse(url)).collect::<Result<Vec<_>, _>>()?;
  auto_download_task_urls(app_handle, task, &download_urls).await?;

  let mut files = Vec::with_capacity(urls.len());
  for (i, url) in urls.iter().enumerate() {
    info!("[FalComplete] Downloading result {} from: {}", i, url);
    files.push(download_file(url, app_data_root, i).await?);
  }

  finish_task_with_local_results(FinishTaskArgs {
    app_handle,
    app_env_configs,
    task_database,
    creds: &creds,
    task,
    files: &files,
    media_class,
    maybe_upload_provider: Some(GenerationProvider::Fal),
  }).await
}

// ── Helpers ──

/// Collect downloadable media URLs from the extracted response contents.
fn collect_media_urls(
  task: &Task,
  extracted: &Option<PollResponseExtractedContents>,
) -> Result<(Vec<String>, TaskMediaFileClass), Box<dyn std::error::Error>> {
  let extracted = match extracted {
    Some(e) => e,
    None => return Ok((vec![], TaskMediaFileClass::Image)),
  };

  // Images (batch)
  if let Some(images) = &extracted.images {
    let urls: Vec<String> = images.iter()
      .filter_map(|img| img.url.clone())
      .collect();
    if !urls.is_empty() {
      return Ok((urls, TaskMediaFileClass::Image));
    }
  }

  // Single image (e.g. background removal)
  if let Some(image) = &extracted.image {
    if let Some(url) = &image.url {
      return Ok((vec![url.clone()], TaskMediaFileClass::Image));
    }
  }

  // Video
  if let Some(video) = &extracted.video {
    if let Some(url) = &video.url {
      return Ok((vec![url.clone()], TaskMediaFileClass::Video));
    }
  }

  // 3D model (GLB)
  if let Some(glb) = &extracted.model_glb {
    if let Some(url) = &glb.url {
      return Ok((vec![url.clone()], TaskMediaFileClass::Dimensional));
    }
  }

  Ok((vec![], TaskMediaFileClass::Image))
}

async fn download_file(
  url: &str,
  app_data_root: &AppDataRoot,
  index: usize,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
  let response = reqwest::get(url).await?;
  let bytes = response.bytes().await?;

  let extension = url_utils::download_extension::extract_download_extension_from_url::extract_download_extension_from_url_str(url)
    .map(|ext| ext.as_extension_without_period())
    .unwrap_or("bin");

  let tempdir = app_data_root.temp_dir().path();
  let filename = format!("fal_{}_{}.{}", generate_random_uuid(), index, extension);
  let download_path = tempdir.join(filename);

  let mut file = File::create(&download_path)?;
  file.write_all(&bytes)?;

  Ok(download_path)
}
