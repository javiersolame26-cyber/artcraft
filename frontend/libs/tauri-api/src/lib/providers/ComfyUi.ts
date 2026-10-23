import { invoke } from "@tauri-apps/api/core";
import { CommandResult } from "../common/CommandStatus";

// Settings for the user's own ComfyUI server (the "comfy_ui" provider).
// Mirrors `ComfyUiSettings` in crates/desktop/artcraft/src/core/state/comfyui_settings.rs.
export interface ComfyUiSettings {
  server_url: string;
  flux_schnell_checkpoint: string;
  flux_dev_checkpoint: string;
  sdxl_checkpoint: string;
  // Flux Kontext's separate files, in models/diffusion_models, models/text_encoders (two) and models/vae.
  kontext_diffusion_model: string;
  kontext_clip_l: string;
  kontext_t5xxl: string;
  kontext_vae: string;
  // 0.05 – 1.0. Lower keeps more of the input image (canvas, 3D scene, reference).
  image_edit_strength: number;
}

export interface ComfyUiDevice {
  name: string;
  device_type: string;
  maybe_vram_total_gib?: number | null;
}

export interface ComfyUiConnectionInfo {
  server_url: string;
  maybe_comfyui_version?: string | null;
  devices: ComfyUiDevice[];
  // Every file in ComfyUI's models/checkpoints folder.
  checkpoints: string[];
  // Configured checkpoints ComfyUI doesn't have.
  missing_checkpoints: string[];
  // Files in models/diffusion_models, models/text_encoders and models/vae.
  diffusion_models: string[];
  text_encoders: string[];
  vae_files: string[];
  // Configured Flux Kontext files ComfyUI doesn't have.
  missing_kontext_files: string[];
}

interface CommandSuccess<T> extends CommandResult {
  payload: T;
}

interface CommandFailure extends CommandResult {
  error_message?: string;
}

// Each call resolves with its payload or throws an Error with a user-facing message.

export const ComfyUiGetSettings = async (): Promise<ComfyUiSettings> => {
  const result = await call<{ settings: ComfyUiSettings }>("comfyui_get_settings_command");
  return result.settings;
};

export const ComfyUiUpdateSettings = async (settings: ComfyUiSettings): Promise<ComfyUiSettings> => {
  const result = await call<{ settings: ComfyUiSettings }>("comfyui_update_settings_command", { request: { settings } });
  return result.settings;
};

// Test an address before saving it; omit it to test the saved address.
export const ComfyUiTestConnection = async (serverUrl?: string): Promise<ComfyUiConnectionInfo> =>
  call<ComfyUiConnectionInfo>("comfyui_test_connection_command", {
    request: { maybe_server_url: serverUrl ?? null },
  });

const call = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
  try {
    const result = (await invoke(command, args)) as CommandSuccess<T>;
    return result.payload;
  } catch (error) {
    const failure = error as CommandFailure | string;
    const message = typeof failure === "string" ? failure : failure?.error_message;
    throw new Error(message || `ComfyUI command ${command} failed`);
  }
};
