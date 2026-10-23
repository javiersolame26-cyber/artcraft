// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { GenerationProvider } from "@storyteller/api-enums";
import { buildImageModelsFromListing } from "../loader/buildModelsFromListing.js";
import { ModelTag } from "../classes/metadata/ModelTag.js";
import { LOCAL_MODELS_FAMILY, getModelFamilyName } from "../classes/metadata/ModelFamilies.js";
import { CommonAspectRatio } from "../classes/properties/CommonAspectRatio.js";
import { IMAGE_MODELS, LOCAL_KONTEXT_MAX_IMAGES, LOCAL_KONTEXT_MODEL_ID } from "./ImageModels.js";

const CHECKPOINT_MODEL_IDS = ["comfyui_flux_1_schnell", "comfyui_flux_1_dev", "comfyui_sdxl"];
const LOCAL_MODEL_IDS = [...CHECKPOINT_MODEL_IDS, LOCAL_KONTEXT_MODEL_ID];

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
      expect(localModel(id).getProviders()).toEqual([GenerationProvider.ComfyUi]);
    }
  });

  it("show up on the text-to-image, canvas and 3D pages", () => {
    for (const id of LOCAL_MODEL_IDS) {
      const model = localModel(id);
      expect(model.canTextToImage).toBe(true);
      expect(model.canEditImages).toBe(true);
      expect(model.tags).toContain(ModelTag.InstructiveEdit);
    }
  });

  it("checkpoint models start from a single image", () => {
    for (const id of CHECKPOINT_MODEL_IDS) {
      expect(localModel(id).maxImagePromptCount).toBe(1);
    }
  });

  it("Kontext takes several images and follows the first one's shape", () => {
    const kontext = localModel(LOCAL_KONTEXT_MODEL_ID);
    expect(kontext.maxImagePromptCount).toBe(LOCAL_KONTEXT_MAX_IMAGES);
    expect(kontext.defaultAspectRatio).toBe(CommonAspectRatio.Auto);
    expect(kontext.maxGenerationCount).toBe(2);
    expect(kontext.defaultGenerationCount).toBe(1);
  });

  it("are grouped together in the model picker", () => {
    for (const id of LOCAL_MODEL_IDS) {
      const model = localModel(id);
      expect(getModelFamilyName(model.id, model.creator)).toBe(LOCAL_MODELS_FAMILY);
    }
    expect(getModelFamilyName("flux_pro_11")).toBe("Flux");
  });
});

function localModel(id: string) {
  const model = IMAGE_MODELS.find((m) => m.tauriId === id);
  if (!model) throw new Error(`missing local model ${id}`);
  return model;
}
