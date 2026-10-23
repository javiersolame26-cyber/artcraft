Local generation with ComfyUI
=============================

ArtCraft can generate images on your own graphics card, for free, through a
[ComfyUI](https://github.com/comfyanonymous/ComfyUI) server running on your
computer or your network. The local models show up in the model picker with a
"· Local" suffix:

| Model                | Family         | Default checkpoint file         | Typical use                   |
|----------------------|----------------|---------------------------------|-------------------------------|
| Flux Schnell · Local | FLUX.1 schnell | `flux1-schnell-fp8.safetensors` | Fast drafts (4 steps)         |
| Flux Dev · Local     | FLUX.1 dev     | `flux1-dev-fp8.safetensors`     | Higher quality, ~20 steps     |
| SDXL · Local         | SDXL           | `sd_xl_base_1.0.safetensors`    | Any SDXL checkpoint/fine-tune |

They work on the Image page (text to image) and in the 2D canvas, the 3D stage
and the image editor. With a canvas, 3D scene or reference image they run
image-to-image from it, so a blocked-out 3D scene keeps its composition.

## Setup

1. Install ComfyUI: the Windows portable build (NVIDIA) or ComfyUI Desktop.
2. Put checkpoint files in `ComfyUI/models/checkpoints`. The FP8 FLUX files
   published by Comfy-Org (`Comfy-Org/flux1-schnell`, `Comfy-Org/flux1-dev` on
   Hugging Face) load with a plain checkpoint loader, which is what ArtCraft uses.
3. Start ComfyUI and note its address. The portable build listens on
   `http://127.0.0.1:8188`; ComfyUI Desktop shows its port in its settings.
4. In ArtCraft open **Settings → ComfyUI (Local)**, enter the address, press
   **Test connection**, choose a file for each model and **Save**.
5. Pick a "· Local" model and generate.

Results are uploaded to your ArtCraft library like any other generation, so you
need to be logged in. Local generations spend no credits.

Check each model's license before using results commercially: FLUX.1 schnell is
Apache 2.0, FLUX.1 dev uses a non-commercial license, and SDXL uses
CreativeML Open RAIL++-M.

### Hardware

| GPU memory | What to expect                                                    |
|------------|-------------------------------------------------------------------|
| 6–8 GB     | SDXL works. FP8 FLUX works with ComfyUI's offloading, but slowly. |
| 12–16 GB   | All three models are comfortable at one or two images per batch.  |
| 24 GB      | Batches of four at full speed.                                    |

A batch renders in one pass. If ComfyUI runs out of memory, generate fewer
images at a time.

## How it works

| Step     | Code                                                                                            |
|----------|-------------------------------------------------------------------------------------------------|
| Request  | `generate_image_command` routes `provider: "comfy_ui"` to `providers/comfyui/handle_comfyui.rs` |
| Workflow | `comfyui_client::workflows::image_workflow` builds the API-format graph per model family        |
| Queue    | `POST /prompt`; the task stores `…/history/{prompt_id}` as its status URL                       |
| Polling  | `third_party_task_polling_thread` → `handlers/comfyui/poll_comfyui_tasks.rs`                    |
| Results  | `GET /view` → temp files → auto-download → `handlers/common/finish_task_with_local_results.rs`  |
| Settings | `settings/comfyui_settings.json`, commands in `core/commands/providers/comfyui/`                |

A task waits while ComfyUI is unreachable (it may be restarting) and fails with
a message when ComfyUI no longer knows the job or reports an error.

## Not covered yet

Masked inpainting, video and image-to-3D still need a cloud provider. They are
the next candidates for local ComfyUI workflows.

## Tests

```bash
cargo test -p comfyui_client
SQLX_OFFLINE=true cargo test -p artcraft --lib -- comfyui auto_download
(cd frontend/libs/model-list && npx vitest run)
```
