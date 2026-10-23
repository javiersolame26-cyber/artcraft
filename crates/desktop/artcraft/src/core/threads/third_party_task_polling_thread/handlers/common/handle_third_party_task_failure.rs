use enums::tauri::tasks::task_status::TaskStatus;
use enums::tauri::tasks::task_type::TaskType;
use log::{error, info};
use sqlite_tasks::queries::task::Task;
use sqlite_tasks::queries::update_task_status::{update_task_status, UpdateTaskArgs};
use tauri::AppHandle;

use crate::core::events::basic_sendable_event_trait::BasicSendableEvent;
use crate::core::events::generation_events::common::GenerationAction;
use crate::core::events::generation_events::generation_failed_event::GenerationFailedEvent;
use crate::core::state::task_database::TaskDatabase;
use crate::core::utils::enum_conversion::generation_provider::to_generation_service_provider;

/// Mark a third-party task as failed and tell the frontend why.
pub async fn handle_third_party_task_failure(
  app_handle: &AppHandle,
  task_database: &TaskDatabase,
  task: &Task,
  reason: &str,
) {
  info!("[ThirdPartyPolling] Marking {:?} task {} as failed: {}", task.provider, task.id.as_str(), reason);

  let update_result = update_task_status(UpdateTaskArgs {
    db: task_database.get_connection(),
    task_id: &task.id,
    status: TaskStatus::CompleteFailure,
  }).await;

  if let Err(err) = update_result {
    error!("[ThirdPartyPolling] Failed to update task status for {}: {:?}", task.id.as_str(), err);
  }

  let event = GenerationFailedEvent {
    action: task_type_to_generation_action(task.task_type),
    service: to_generation_service_provider(task.provider),
    model: None,
    reason: Some(reason.to_string()),
  };

  event.send_infallible(app_handle);
}

fn task_type_to_generation_action(task_type: TaskType) -> GenerationAction {
  match task_type {
    TaskType::ImageGeneration => GenerationAction::GenerateImage,
    TaskType::VideoGeneration => GenerationAction::GenerateVideo,
    TaskType::AudioGeneration => GenerationAction::GenerateAudio,
    TaskType::BackgroundRemoval => GenerationAction::RemoveBackground,
    TaskType::ObjectGeneration => GenerationAction::ImageTo3d,
    TaskType::GaussianGeneration => GenerationAction::GenerateGaussian,
    TaskType::ImageInpaintEdit => GenerationAction::ImageInpaintEdit,
  }
}
