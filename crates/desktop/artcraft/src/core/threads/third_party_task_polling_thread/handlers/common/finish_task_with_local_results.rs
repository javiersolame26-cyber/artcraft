use std::path::PathBuf;
use std::time::Duration;

use artcraft_api_defs::utils::media_links_to_thumbnail_template::media_links_to_thumbnail_template;
use artcraft_client::credentials::storyteller_credential_set::StorytellerCredentialSet;
use artcraft_client::endpoints::media_files::get_media_file::get_media_file;
use artcraft_client::endpoints::media_files::legacy_upload_media_file_from_file::{
  legacy_upload_media_file_from_file, LegacyUploadMediaFileFromFileArgs,
};
use artcraft_client::endpoints::media_files::upload_image_media_file_from_file::{
  upload_image_media_file_from_file, UploadImageFromFileArgs,
};
use artcraft_client::endpoints::media_files::upload_video_media_file_from_file::{
  upload_video_media_file_from_file, UploadVideoFromFileArgs,
};
use artcraft_client::error::api_error::ApiError;
use artcraft_client::error::storyteller_error::StorytellerError;
use enums::common::generation_provider::GenerationProvider;
use enums::tauri::tasks::task_media_file_class::TaskMediaFileClass;
use log::{error, info, warn};
use sqlite_tasks::queries::task::Task;
use sqlite_tasks::queries::update_successful_task_status_with_metadata::{
  update_successful_task_status_with_metadata, UpdateSuccessfulTaskArgs,
};
use tauri::AppHandle;
use tokens::tokens::batch_generations::BatchGenerationToken;
use tokens::tokens::media_files::MediaFileToken;
use tokens::tokens::prompts::PromptToken;

use crate::core::state::app_env_configs::app_env_configs::AppEnvConfigs;
use crate::core::state::task_database::TaskDatabase;
use crate::core::threads::third_party_task_polling_thread::events::notify_frontend_of_completion::{
  notify_frontend_of_completion, CompletionData,
};
use crate::core::utils::auto_download::clear_auto_download_checkpoint;

const MAX_UPLOAD_RETRIES: u32 = 5;
const INITIAL_RETRY_DELAY_SECS: u64 = 10;

pub struct FinishTaskArgs<'a> {
  pub app_handle: &'a AppHandle,
  pub app_env_configs: &'a AppEnvConfigs,
  pub task_database: &'a TaskDatabase,
  pub creds: &'a StorytellerCredentialSet,
  pub task: &'a Task,

  /// Finished results, already downloaded and already auto-saved for the user.
  pub files: &'a [PathBuf],
  pub media_class: TaskMediaFileClass,

  /// Reported to the ArtCraft backend on upload. `None` for providers the backend doesn't know.
  pub maybe_upload_provider: Option<GenerationProvider>,
}

/// Upload a third-party task's results to the ArtCraft library, mark the
/// task complete, and tell the frontend. An error leaves the task pending
/// so the next polling pass retries.
pub async fn finish_task_with_local_results(args: FinishTaskArgs<'_>) -> Result<(), Box<dyn std::error::Error>> {
  let task = args.task;

  // Use the prompt token from the task record (created at generation time).
  let maybe_prompt_token = task.prompt_token.as_ref()
    .map(|s| PromptToken::new_from_str(s));

  if maybe_prompt_token.is_none() {
    warn!("[TaskComplete] Task {} has no prompt token, uploading without prompt association", task.id.as_str());
  }

  let maybe_batch_token = if args.files.len() > 1 {
    let token = BatchGenerationToken::generate();
    info!("[TaskComplete] Using batch token for {} files: {:?}", args.files.len(), token);
    Some(token)
  } else {
    None
  };

  let mut maybe_primary_media_file_token: Option<MediaFileToken> = None;

  for (i, path) in args.files.iter().enumerate() {
    info!("[TaskComplete] Uploading result {} of task {} to backend...", i, task.id.as_str());

    let media_token = upload_to_backend(
      args.creds,
      args.app_env_configs,
      path,
      maybe_prompt_token.as_ref(),
      maybe_batch_token.as_ref(),
      args.media_class,
      args.maybe_upload_provider,
    ).await?;

    info!("[TaskComplete] Uploaded as media file: {:?}", media_token);

    if maybe_primary_media_file_token.is_none() {
      maybe_primary_media_file_token = Some(media_token);
    }
  }

  // Look up CDN/thumbnail URLs for the primary media file
  let mut maybe_cdn_url: Option<reqwest::Url> = None;
  let mut maybe_cdn_url_str: Option<String> = None;
  let mut maybe_thumbnail_url_template = None;

  if let Some(media_file_token) = maybe_primary_media_file_token.as_ref() {
    match get_media_file(&args.app_env_configs.storyteller_host, media_file_token).await {
      Ok(response) => {
        maybe_cdn_url = Some(response.media_file.media_links.cdn_url.clone());
        maybe_cdn_url_str = Some(response.media_file.media_links.cdn_url.to_string());
        maybe_thumbnail_url_template = media_links_to_thumbnail_template(&response.media_file.media_links)
          .map(|s| s.to_string());
      }
      Err(err) => {
        error!("[TaskComplete] Failed to look up media file after upload: {:?} (failing open)", err);
      }
    }
  }

  // Mark the task as completed in the local database
  let updated = update_successful_task_status_with_metadata(UpdateSuccessfulTaskArgs {
    db: args.task_database.get_connection(),
    task_id: &task.id,
    maybe_batch_token: maybe_batch_token.as_ref(),
    maybe_primary_media_file_token: maybe_primary_media_file_token.as_ref(),
    maybe_primary_media_file_class: Some(args.media_class),
    maybe_primary_media_file_cdn_url: maybe_cdn_url_str.as_deref(),
    maybe_primary_media_file_thumbnail_url_template: maybe_thumbnail_url_template.as_deref(),
  }).await?;

  if updated {
    clear_auto_download_checkpoint(args.app_handle, task);
    if let Some(primary_token) = maybe_primary_media_file_token {
      let completion = CompletionData {
        primary_media_file_token: primary_token,
        maybe_cdn_url,
        maybe_thumbnail_url_template,
        maybe_batch_token,
        media_class: args.media_class,
      };

      notify_frontend_of_completion(
        args.app_handle,
        &args.app_env_configs.storyteller_host,
        Some(args.creds),
        task,
        &completion,
      ).await;
    }
  }

  info!("[TaskComplete] Task {} fully handled", task.id.as_str());

  Ok(())
}

async fn upload_to_backend(
  creds: &StorytellerCredentialSet,
  app_env_configs: &AppEnvConfigs,
  path: &PathBuf,
  maybe_prompt_token: Option<&PromptToken>,
  maybe_batch_token: Option<&BatchGenerationToken>,
  media_class: TaskMediaFileClass,
  maybe_provider: Option<GenerationProvider>,
) -> Result<MediaFileToken, Box<dyn std::error::Error>> {
  let mut retry_delay_secs = INITIAL_RETRY_DELAY_SECS;

  for attempt in 0..MAX_UPLOAD_RETRIES {
    let result = try_upload(creds, app_env_configs, path, maybe_prompt_token, maybe_batch_token, media_class, maybe_provider).await;

    match result {
      Ok(token) => return Ok(token),
      Err(StorytellerError::Api(ApiError::TooManyRequests(_))) => {
        if attempt + 1 < MAX_UPLOAD_RETRIES {
          warn!(
            "[TaskComplete] Upload rate-limited (429), retrying in {}s (attempt {}/{})",
            retry_delay_secs,
            attempt + 1,
            MAX_UPLOAD_RETRIES,
          );
          tokio::time::sleep(Duration::from_secs(retry_delay_secs)).await;
          retry_delay_secs = (retry_delay_secs * 2).min(60);
        } else {
          error!("[TaskComplete] Upload rate-limited after {} attempts, giving up", MAX_UPLOAD_RETRIES);
          return Err(Box::new(StorytellerError::Api(ApiError::TooManyRequests(
            "Max retries exceeded".to_string(),
          ))));
        }
      }
      Err(err) => return Err(Box::new(err)),
    }
  }

  unreachable!()
}

async fn try_upload(
  creds: &StorytellerCredentialSet,
  app_env_configs: &AppEnvConfigs,
  path: &PathBuf,
  maybe_prompt_token: Option<&PromptToken>,
  maybe_batch_token: Option<&BatchGenerationToken>,
  media_class: TaskMediaFileClass,
  maybe_provider: Option<GenerationProvider>,
) -> Result<MediaFileToken, StorytellerError> {
  let media_token = match media_class {
    TaskMediaFileClass::Video => {
      let result = upload_video_media_file_from_file(UploadVideoFromFileArgs {
        api_host: &app_env_configs.storyteller_host,
        maybe_creds: Some(creds),
        path,
        maybe_prompt_token,
        maybe_generation_provider: maybe_provider,
      }).await?;
      result.media_file_token
    }
    TaskMediaFileClass::Dimensional => {
      let result = legacy_upload_media_file_from_file(LegacyUploadMediaFileFromFileArgs {
        api_host: &app_env_configs.storyteller_host,
        maybe_creds: Some(creds),
        path,
        maybe_generation_provider: maybe_provider,
      }).await?;
      result.media_file_token
    }
    _ => {
      let result = upload_image_media_file_from_file(UploadImageFromFileArgs {
        api_host: &app_env_configs.storyteller_host,
        maybe_creds: Some(creds),
        path,
        is_intermediate_system_file: false,
        maybe_prompt_token,
        maybe_batch_token,
        maybe_generation_provider: maybe_provider,
      }).await?;
      result.media_file_token
    }
  };

  Ok(media_token)
}
