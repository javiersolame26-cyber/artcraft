// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { GenerationProvider } from "@storyteller/api-enums";
import { buildImageModelsFromListing } from "../loader/buildModelsFromListing.js";
import { ModelTag } from "../classes/metadata/ModelTag.js";
import { IMAGE_MODELS } from "./ImageModels.js";

const LOCAL_MODEL_IDS = ["comfyui_flux_1_schnell", "comfyui_flux_1_dev", "comfyui_sdxl"];

describe("local ComfyUI image models", () => {
  it("survive a backend listing that doesn't know them", () => {
    const models = buildImageModelsFromListing(IMAGE_MODELS, [{ model: "nano_banana_pro" }]);
    const ids = models.map((model) => model.tauriId);
    for (const id of LOCAL_MODEL_IDS) {
      expect(ids).toContain(id);
    }
  });

  it("only run on the ComfyUI provider", () => {
    for (const id of LOCAL_MODEL_IDS) {
      const model = IMAGE_MODELS.find((m) => m.tauriId === id);
      expect(model?.getProviders()).toEqual([GenerationProvider.ComfyUi]);
    }
  });

  it("show up on the text-to-image, canvas and 3D pages", () => {
    for (const id of LOCAL_MODEL_IDS) {
      const model = IMAGE_MODELS.find((m) => m.tauriId === id)!;
      expect(model.canTextToImage).toBe(true);
      expect(model.canEditImages).toBe(true);
      expect(model.tags).toContain(ModelTag.InstructiveEdit);
      expect(model.maxImagePromptCount).toBe(1);
    }
  });
});
