use comfyui_client::api::history::{OutputImage, PromptHistory, PromptStatus};
use comfyui_client::api::queue::QueueSnapshot;
use comfyui_client::client::ComfyUiClient;
use log::{error, info};
use sqlite_tasks::queries::task::Task;
use tauri::AppHandle;

use crate::core::state::app_env_configs::app_env_configs::AppEnvConfigs;
use crate::core::state::data_dir::app_data_root::AppDataRoot;
use crate::core::state::task_database::TaskDatabase;
use crate::core::threads::third_party_task_polling_thread::handlers::comfyui::handle_comfyui_complete::handle_comfyui_complete;
use crate::core::threads::third_party_task_polling_thread::handlers::common::handle_third_party_task_failure::handle_third_party_task_failure;
use crate::services::storyteller::state::storyteller_credential_manager::StorytellerCredentialManager;

const LOST_JOB_REASON: &str = "ComfyUI no longer has this job. It may have been restarted or its queue cleared. Generate again.";

/// What a task needs after looking at ComfyUI.
#[derive(Debug, PartialEq)]
enum NextStep {
  KeepWaiting,
  Complete(Vec<OutputImage>),
  Fail(String),
}

/// Check every pending ComfyUI task once.
///
/// An unreachable server is not a failure: the user may have closed ComfyUI
/// for a moment, so the task waits. A server that is up but no longer knows
/// the job (restarted, queue cleared) fails it.
pub async fn poll_comfyui_tasks(
  app_handle: &AppHandle,
  app_env_configs: &AppEnvConfigs,
  app_data_root: &AppDataRoot,
  task_database: &TaskDatabase,
  storyteller_creds_manager: &StorytellerCredentialManager,
  tasks: &[&Task],
) {
  for task in tasks {
    let result = poll_single_comfyui_task(
      app_handle,
      app_env_configs,
      app_data_root,
      task_database,
      storyteller_creds_manager,
      task,
    ).await;

    if let Err(err) = result {
      error!("[ComfyUiPolling] Error processing task {}: {:?}", task.id.as_str(), err);
    }
  }
}

async fn poll_single_comfyui_task(
  app_handle: &AppHandle,
  app_env_configs: &AppEnvConfigs,
  app_data_root: &AppDataRoot,
  task_database: &TaskDatabase,
  storyteller_creds_manager: &StorytellerCredentialManager,
  task: &Task,
) -> Result<(), Box<dyn std::error::Error>> {
  let client_and_prompt = task.queue_status_url.as_deref()
    .map(ComfyUiClient::for_history_url);

  let (client, prompt_id) = match client_and_prompt {
    Some(Ok(client_and_prompt)) => client_and_prompt,
    _ => {
      handle_third_party_task_failure(app_handle, task_database, task, "The ComfyUI job address is missing or invalid.").await;
      return Ok(());
    }
  };

  let next_step = match check_job(&client, &prompt_id).await {
    Ok(step) => step,
    Err(err) if err.is_unreachable() => {
      info!("[ComfyUiPolling] ComfyUI at {} is not reachable; task {} keeps waiting", client.base_url(), task.id.as_str());
      return Ok(());
    }
    Err(err) => return Err(Box::new(err)),
  };

  match next_step {
    NextStep::KeepWaiting => {
      info!("[ComfyUiPolling] Task {} (prompt {}) is still in ComfyUI's queue", task.id.as_str(), prompt_id);
    }
    NextStep::Fail(reason) => {
      handle_third_party_task_failure(app_handle, task_database, task, &reason).await;
    }
    NextStep::Complete(images) => {
      info!("[ComfyUiPolling] Task {} finished with {} image(s)", task.id.as_str(), images.len());
      handle_comfyui_complete(
        app_handle,
        app_env_configs,
        app_data_root,
        task_database,
        storyteller_creds_manager,
        task,
        &client,
        &images,
      ).await;
    }
  }

  Ok(())
}

/// History first; only when there is none, look at the queue. A job can
/// finish between the two calls, so history is read once more before
/// declaring it lost.
async fn check_job(client: &ComfyUiClient, prompt_id: &str) -> Result<NextStep, comfyui_client::error::ComfyUiError> {
  if let Some(history) = client.get_history(prompt_id).await? {
    return Ok(next_step_from_history(history));
  }
  let queue = client.get_queue().await?;
  if queue.contains(prompt_id) {
    return Ok(NextStep::KeepWaiting);
  }
  match client.get_history(prompt_id).await? {
    Some(history) => Ok(next_step_from_history(history)),
    None => Ok(next_step_without_history(&queue, prompt_id)),
  }
}

fn next_step_from_history(history: PromptHistory) -> NextStep {
  match history.status {
    PromptStatus::Success if history.output_images.is_empty() => {
      NextStep::Fail("ComfyUI finished but did not save any image.".to_string())
    }
    PromptStatus::Success => NextStep::Complete(history.output_images),
    PromptStatus::Error { message } => NextStep::Fail(format!("ComfyUI failed: {}", message)),
    PromptStatus::Incomplete { status } => {
      NextStep::Fail(format!("ComfyUI stopped without finishing (status: {}).", if status.is_empty() { "unknown" } else { &status }))
    }
  }
}

fn next_step_without_history(queue: &QueueSnapshot, prompt_id: &str) -> NextStep {
  if queue.contains(prompt_id) {
    NextStep::KeepWaiting
  } else {
    NextStep::Fail(LOST_JOB_REASON.to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const PROMPT_ID: &str = "prompt-1";

  mod history_tests {
    use super::*;

    #[test]
    fn success_completes_with_images() {
      let step = next_step_from_history(PromptHistory { status: PromptStatus::Success, output_images: vec![image("a.png")] });
      assert_eq!(step, NextStep::Complete(vec![image("a.png")]));
    }

    #[test]
    fn success_without_images_fails() {
      let step = next_step_from_history(PromptHistory { status: PromptStatus::Success, output_images: vec![] });
      assert!(matches!(step, NextStep::Fail(_)));
    }

    #[test]
    fn error_message_is_passed_on() {
      let step = next_step_from_history(PromptHistory {
        status: PromptStatus::Error { message: "CUDA out of memory (node KSampler)".to_string() },
        output_images: vec![],
      });
      assert_eq!(step, NextStep::Fail("ComfyUI failed: CUDA out of memory (node KSampler)".to_string()));
    }

    #[test]
    fn incomplete_fails() {
      let step = next_step_from_history(PromptHistory { status: PromptStatus::Incomplete { status: String::new() }, output_images: vec![] });
      assert_eq!(step, NextStep::Fail("ComfyUI stopped without finishing (status: unknown).".to_string()));
    }
  }

  mod queue_tests {
    use super::*;

    #[test]
    fn queued_job_keeps_waiting() {
      let queue = QueueSnapshot { running: vec![], pending: vec![PROMPT_ID.to_string()] };
      assert_eq!(next_step_without_history(&queue, PROMPT_ID), NextStep::KeepWaiting);
    }

    #[test]
    fn unknown_job_is_lost() {
      assert_eq!(next_step_without_history(&QueueSnapshot::default(), PROMPT_ID), NextStep::Fail(LOST_JOB_REASON.to_string()));
    }
  }

  fn image(filename: &str) -> OutputImage {
    OutputImage { filename: filename.to_string(), subfolder: "ArtCraft".to_string(), folder_type: "output".to_string() }
  }
}
