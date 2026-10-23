import { beforeEach, describe, expect, it } from "vitest";
import { CommonAspectRatio } from "@storyteller/model-list";
import { FUSE_MAX_IMAGES, FuseTurn, applyCompletion, applyFailure, useFuseStore } from "./FuseStore";

const RESULT = { mediaToken: "m_result", url: "https://cdn.example.com/result.png" };

const initialState = useFuseStore.getInitialState();

beforeEach(() => {
  useFuseStore.setState(initialState, true);
});

describe("turn events", () => {
  it("completes only the matching working turn", () => {
    const turns = [turn("a"), turn("b")];
    const next = applyCompletion(turns, "b", [RESULT]);
    expect(next[0]).toBe(turns[0]);
    expect(next[1].status).toBe("done");
    expect(next[1].results).toEqual([RESULT]);
  });

  it("ignores events for other pages and finished turns", () => {
    const turns = [turn("a"), { ...turn("b"), status: "failed" as const }];
    expect(applyCompletion(turns, "someone-else", [RESULT])).toBe(turns);
    expect(applyFailure(turns, "b", "late failure")).toBe(turns);
  });

  it("records the failure reason", () => {
    const next = applyFailure([turn("a")], "a", "Could not reach ComfyUI");
    expect(next[0].status).toBe("failed");
    expect(next[0].maybeError).toBe("Could not reach ComfyUI");
  });
});

describe("attachments", () => {
  it("adds library images once and stops at the limit", () => {
    const store = useFuseStore.getState();
    expect(store.attachLibraryImage(image(1))).toBe(true);
    expect(store.attachLibraryImage(image(1))).toBe(true);
    expect(useFuseStore.getState().attachments).toHaveLength(1);
    for (let i = 2; i <= FUSE_MAX_IMAGES; i++) store.attachLibraryImage(image(i));
    expect(useFuseStore.getState().attachLibraryImage(image(99))).toBe(false);
    expect(useFuseStore.getState().attachments).toHaveLength(FUSE_MAX_IMAGES);
  });

  it("continuing from a result makes it image 1 and keeps the others", () => {
    const store = useFuseStore.getState();
    store.attachLibraryImage(image(1));
    store.attachLibraryImage(image(2));
    store.continueFrom(RESULT);
    const tokens = useFuseStore.getState().attachments.map((a) => a.mediaToken);
    expect(tokens).toEqual(["m_result", "m_1", "m_2"]);
  });
});

function turn(id: string): FuseTurn {
  return {
    id,
    instruction: "put the cat on the sofa",
    inputs: [],
    aspectRatio: CommonAspectRatio.Auto,
    status: "working",
    results: [],
    createdAt: 0,
  };
}

function image(n: number) {
  return { mediaToken: `m_${n}`, url: `https://cdn.example.com/${n}.png` };
}
