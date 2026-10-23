import { useEffect, useMemo, useState } from "react";
import { Button } from "@storyteller/ui-button";
import { Label } from "@storyteller/ui-label";
import { Select } from "@storyteller/ui-select";
import {
  ComfyUiConnectionInfo,
  ComfyUiGetSettings,
  ComfyUiSettings,
  ComfyUiTestConnection,
  ComfyUiUpdateSettings,
} from "@storyteller/tauri-api";
import { PlugZapIcon, SaveIcon } from "lucide-react";

type CheckpointField = "flux_schnell_checkpoint" | "flux_dev_checkpoint" | "sdxl_checkpoint";

const CHECKPOINT_FIELDS: { field: CheckpointField; model: string; hint: string }[] = [
  { field: "flux_schnell_checkpoint", model: "Flux Schnell · Local", hint: "e.g. flux1-schnell-fp8.safetensors" },
  { field: "flux_dev_checkpoint", model: "Flux Dev · Local", hint: "e.g. flux1-dev-fp8.safetensors" },
  { field: "sdxl_checkpoint", model: "SDXL · Local", hint: "any SDXL checkpoint, e.g. sd_xl_base_1.0.safetensors" },
];

const INPUT_CLASS =
  "min-w-0 flex-1 rounded-[3px] border border-ui-controls-border bg-ui-controls px-3 py-2 font-mono text-xs outline-none focus:border-base-fg/40";

export const ComfyUiSettingsPane = () => {
  const [settings, setSettings] = useState<ComfyUiSettings | undefined>(undefined);
  const [connection, setConnection] = useState<ComfyUiConnectionInfo | undefined>(undefined);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    ComfyUiGetSettings()
      .then((loaded) => {
        setSettings(loaded);
        return testConnection(loaded.server_url);
      })
      .catch((error) => setSaveError(String(error)));
    // Load once when the pane opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const testConnection = async (serverUrl: string) => {
    setTesting(true);
    setConnectionError(null);
    try {
      setConnection(await ComfyUiTestConnection(serverUrl));
    } catch (error) {
      setConnection(undefined);
      setConnectionError(error instanceof Error ? error.message : String(error));
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    if (!settings) return;
    setSaving(true);
    setSaveError(null);
    setSaveMessage(null);
    try {
      const saved = await ComfyUiUpdateSettings(settings);
      setSettings(saved);
      setSaveMessage("Saved. Local models use these settings from the next generation.");
      await testConnection(saved.server_url);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const update = <K extends keyof ComfyUiSettings>(key: K, value: ComfyUiSettings[K]) => {
    setSaveMessage(null);
    setSettings((current) => (current ? { ...current, [key]: value } : current));
  };

  const installed = useMemo(() => new Set(connection?.checkpoints ?? []), [connection]);

  if (!settings) {
    return (
      <div className="space-y-4 pt-3 text-base-fg">
        {saveError ? <p role="alert" className="text-red-400">{saveError}</p> : <p className="text-xs opacity-70">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="space-y-4 pt-3 text-base-fg">
      <p className="text-xs opacity-70">
        Generate images for free on your own graphics card. The local models
        (Flux Schnell, Flux Dev and SDXL, marked “Local”) run on a ComfyUI
        server on this computer or your network. Results go to your library
        like any other generation. With a canvas, 3D scene or reference image
        they start from that image.
      </p>

      <div className="flex flex-col gap-2">
        <Label htmlFor="comfyui-server-url">ComfyUI address</Label>
        <div className="flex flex-wrap items-center gap-2">
          <input
            id="comfyui-server-url"
            type="text"
            value={settings.server_url}
            onChange={(event) => update("server_url", event.target.value)}
            placeholder="http://127.0.0.1:8188"
            spellCheck={false}
            className={INPUT_CLASS}
          />
          <Button variant="secondary" onClick={() => testConnection(settings.server_url)} disabled={testing}>
            <PlugZapIcon />
            {testing ? "Testing…" : "Test connection"}
          </Button>
        </div>
        <ConnectionStatus connection={connection} error={connectionError} testing={testing} />
      </div>

      <fieldset className="flex flex-col gap-3 border-0 p-0">
        <div className="flex flex-col gap-0.5">
          <Label>Model files</Label>
          <p className="text-xs opacity-70">
            Checkpoint files in ComfyUI/models/checkpoints. The FP8 Flux
            checkpoints from Comfy-Org load directly.
          </p>
        </div>
        {CHECKPOINT_FIELDS.map(({ field, model, hint }) => (
          <div key={field} className="flex flex-col gap-1">
            <Label htmlFor={`comfyui-${field}`}>{model}</Label>
            {connection && connection.checkpoints.length > 0 ? (
              <div className="max-w-md">
                <Select
                  id={`comfyui-${field}`}
                  value={settings[field]}
                  options={checkpointOptions(connection.checkpoints, settings[field])}
                  onChange={(value) => update(field, String(value))}
                />
              </div>
            ) : (
              <input
                id={`comfyui-${field}`}
                type="text"
                value={settings[field]}
                onChange={(event) => update(field, event.target.value)}
                placeholder={hint}
                spellCheck={false}
                className={INPUT_CLASS}
              />
            )}
            {connection && !installed.has(settings[field]) && (
              <p className="text-xs text-amber-400">
                ComfyUI doesn't have this file. {model} will fail until you add it or pick another file.
              </p>
            )}
          </div>
        ))}
      </fieldset>

      <div className="flex flex-col gap-2">
        <div className="flex flex-col gap-0.5">
          <Label htmlFor="comfyui-edit-strength">
            Input image strength: {Math.round(settings.image_edit_strength * 100)}%
          </Label>
          <p className="text-xs opacity-70">
            How much a canvas, 3D scene or reference image may change. Lower
            keeps the composition; higher follows the prompt more.
          </p>
        </div>
        <input
          id="comfyui-edit-strength"
          type="range"
          min={0.05}
          max={1}
          step={0.05}
          value={settings.image_edit_strength}
          onChange={(event) => update("image_edit_strength", Number(event.target.value))}
          className="w-full max-w-md accent-primary"
        />
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <Button variant="primary" onClick={save} disabled={saving}>
          <SaveIcon />
          {saving ? "Saving…" : "Save"}
        </Button>
        {saveMessage && <p className="text-xs opacity-70">{saveMessage}</p>}
      </div>
      {saveError && <p role="alert" className="text-xs text-red-400">{saveError}</p>}
    </div>
  );
};

const ConnectionStatus = ({
  connection,
  error,
  testing,
}: {
  connection: ComfyUiConnectionInfo | undefined;
  error: string | null;
  testing: boolean;
}) => {
  if (testing) return null;
  if (error) {
    return (
      <div role="alert" className="border border-red-400/40 bg-ui-sunken px-3 py-2 text-xs text-red-400">
        {error}
      </div>
    );
  }
  if (!connection) return null;
  const gpus = connection.devices
    .filter((device) => device.device_type !== "cpu")
    .map((device) => describeDevice(device.name, device.maybe_vram_total_gib));
  return (
    <div className="border border-ui-panel-border bg-ui-sunken px-3 py-2 text-xs">
      <p className="text-green-400">
        Connected{connection.maybe_comfyui_version ? ` · ComfyUI ${connection.maybe_comfyui_version}` : ""}
      </p>
      <p className="opacity-70">
        {gpus.length > 0 ? gpus.join(", ") : "No GPU reported. ComfyUI will run on the CPU, which is very slow."}
      </p>
      <p className="opacity-70">
        {connection.checkpoints.length === 1
          ? "1 checkpoint installed"
          : `${connection.checkpoints.length} checkpoints installed`}
      </p>
    </div>
  );
};

// "cuda:0 NVIDIA GeForce RTX 3060 : cudaMallocAsync" → "NVIDIA GeForce RTX 3060 (12 GB)"
const describeDevice = (name: string, vramGib?: number | null): string => {
  const cleaned = name.replace(/^cuda:\d+\s*/, "").replace(/\s*:\s*\w+$/, "").trim() || name;
  return vramGib ? `${cleaned} (${vramGib} GB)` : cleaned;
};

const checkpointOptions = (installed: string[], current: string) => {
  const options = installed.map((name) => ({ label: name, value: name }));
  if (current && !installed.includes(current)) {
    options.unshift({ label: `${current} (not found)`, value: current });
  }
  return options;
};
