use std::path::{Path, PathBuf};

use comfyui_client::api::history::OutputImage;
use comfyui_client::client::ComfyUiClient;
use enums::tauri::tasks::task_media_file_class::TaskMediaFileClass;
use log::{error, info};
use sqlite_tasks::queries::task::Task;
use tauri::AppHandle;
use url::Url;

use crate::core::state::app_env_configs::app_env_configs::AppEnvConfigs;
use crate::core::state::data_dir::app_data_root::AppDataRoot;
use crate::core::state::data_dir::trait_data_subdir::DataSubdir;
use crate::core::state::task_database::TaskDatabase;
use crate::core::threads::third_party_task_polling_thread::handlers::common::finish_task_with_local_results::{
  finish_task_with_local_results, FinishTaskArgs,
};
use crate::core::utils::auto_download::auto_download_task_urls;
use crate::services::storyteller::state::storyteller_credential_manager::StorytellerCredentialManager;

/// Download a finished ComfyUI job's images and file them like any other result.
pub async fn handle_comfyui_complete(
  app_handle: &AppHandle,
  app_env_configs: &AppEnvConfigs,
  app_data_root: &AppDataRoot,
  task_database: &TaskDatabase,
  storyteller_creds_manager: &StorytellerCredentialManager,
  task: &Task,
  client: &ComfyUiClient,
  images: &[OutputImage],
) {
  let result = handle_comfyui_complete_inner(
    app_handle,
    app_env_configs,
    app_data_root,
    task_database,
    storyteller_creds_manager,
    task,
    client,
    images,
  ).await;

  if let Err(err) = result {
    error!("[ComfyUiComplete] Failed to handle task {} (will retry): {:?}", task.id.as_str(), err);
  }
}

async fn handle_comfyui_complete_inner(
  app_handle: &AppHandle,
  app_env_configs: &AppEnvConfigs,
  app_data_root: &AppDataRoot,
  task_database: &TaskDatabase,
  storyteller_creds_manager: &StorytellerCredentialManager,
  task: &Task,
  client: &ComfyUiClient,
  images: &[OutputImage],
) -> Result<(), Box<dyn std::error::Error>> {
  let creds = storyteller_creds_manager.get_credentials()?
    .ok_or("Log into ArtCraft to save local results to your library")?;

  let mut files = Vec::with_capacity(images.len());
  for (index, image) in images.iter().enumerate() {
    let path = result_path(app_data_root.temp_dir().path(), task, index, image);
    // Retries after a failed upload reuse what is already on disk.
    if !path.is_file() {
      info!("[ComfyUiComplete] Downloading {} for task {}", image.filename, task.id.as_str());
      let bytes = client.download_image(image).await?;
      tokio::fs::write(&path, bytes).await?;
    }
    files.push(path);
  }

  // Save copies for the user (if enabled) before uploading or announcing completion.
  let file_urls = files.iter()
    .map(|path| Url::from_file_path(path).map_err(|_| format!("Invalid result path {:?}", path)))
    .collect::<Result<Vec<_>, _>>()?;
  auto_download_task_urls(app_handle, task, &file_urls).await?;

  finish_task_with_local_results(FinishTaskArgs {
    app_handle,
    app_env_configs,
    task_database,
    creds: &creds,
    task,
    files: &files,
    media_class: TaskMediaFileClass::Image,
    // The ArtCraft backend doesn't know local providers.
    maybe_upload_provider: None,
  }).await
}

/// Stable per task and index, so a retry finds the file it already downloaded
/// and the auto-download checkpoint doesn't save a second copy.
fn result_path(temp_dir: &Path, task: &Task, index: usize, image: &OutputImage) -> PathBuf {
  let extension = Path::new(&image.filename)
    .extension()
    .and_then(|ext| ext.to_str())
    .filter(|ext| ext.chars().all(|c| c.is_ascii_alphanumeric()))
    .unwrap_or("png");
  temp_dir.join(format!("comfyui_{}_{}.{}", task.id.as_str(), index, extension))
}

#[cfg(test)]
mod tests {
  use enums::common::generation_provider::GenerationProvider;
  use enums::tauri::tasks::task_status::TaskStatus;
  use enums::tauri::tasks::task_type::TaskType;
  use tokens::tokens::sqlite::tasks::TaskId;

  use super::*;

  #[test]
  fn result_path_is_stable_and_keeps_extension() {
    let task = task();
    let temp = Path::new("/tmp/artcraft");
    let image = OutputImage { filename: "artcraft_00003_.webp".into(), subfolder: "ArtCraft".into(), folder_type: "output".into() };

    let first = result_path(temp, &task, 1, &image);

    assert_eq!(first, result_path(temp, &task, 1, &image));
    assert_eq!(first.extension().unwrap(), "webp");
    assert!(first.file_name().unwrap().to_str().unwrap().starts_with("comfyui_"));
  }

  #[test]
  fn odd_extensions_fall_back_to_png() {
    let image = OutputImage { filename: "no_extension".into(), subfolder: String::new(), folder_type: "output".into() };
    assert_eq!(result_path(Path::new("/tmp"), &task(), 0, &image).extension().unwrap(), "png");
  }

  fn task() -> Task {
    Task {
      id: TaskId::generate(),
      status: TaskStatus::Pending,
      task_type: TaskType::ImageGeneration,
      model_type: None,
      provider: GenerationProvider::ComfyUi,
      provider_job_id: Some("prompt-1".to_string()),
      queue_status_url: Some("http://127.0.0.1:8188/history/prompt-1".to_string()),
      queue_response_url: None,
      prompt_token: None,
      frontend_caller: None,
      frontend_subscriber_id: None,
      frontend_subscriber_payload: None,
    }
  }
}
