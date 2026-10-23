import { create } from "zustand";
import type { RefImage } from "@storyteller/ui-promptbox";
import { CommonAspectRatio, LOCAL_KONTEXT_MAX_IMAGES } from "@storyteller/model-list";

// The Fuse page is a small chat: each message is an instruction plus up to
// four images, answered by Flux Kontext running on the user's GPU. The
// conversation lives in this store so it survives switching tabs.

export const FUSE_MAX_IMAGES = LOCAL_KONTEXT_MAX_IMAGES;

export type FuseTurnStatus = "working" | "done" | "failed";

export interface FuseResult {
  mediaToken: string;
  url: string;
}

export interface FuseTurn {
  // Also the generation's frontend subscriber id, which ties events back to it.
  id: string;
  instruction: string;
  inputs: FuseResult[];
  aspectRatio: CommonAspectRatio;
  status: FuseTurnStatus;
  results: FuseResult[];
  maybeError?: string;
  createdAt: number;
}

interface FuseState {
  // The images attached to the next message, in order (image 1 is leftmost).
  attachments: RefImage[];
  instruction: string;
  aspectRatio: CommonAspectRatio;
  turns: FuseTurn[];

  setAttachments: (attachments: RefImage[]) => void;
  setInstruction: (instruction: string) => void;
  setAspectRatio: (aspectRatio: CommonAspectRatio) => void;
  // Attach an image that's already in the library (from the lightbox, or a result).
  attachLibraryImage: (image: FuseResult) => boolean;
  // Start the next message from a result: it becomes image 1, the rest stay.
  continueFrom: (result: FuseResult) => void;
  startTurn: (turn: FuseTurn) => void;
  completeTurn: (id: string, results: FuseResult[]) => boolean;
  failTurn: (id: string, reason: string) => boolean;
  clearConversation: () => void;
}

export const useFuseStore = create<FuseState>()((set, get) => ({
  attachments: [],
  instruction: "",
  aspectRatio: CommonAspectRatio.Auto,
  turns: [],

  setAttachments: (attachments) => set({ attachments }),
  setInstruction: (instruction) => set({ instruction }),
  setAspectRatio: (aspectRatio) => set({ aspectRatio }),

  attachLibraryImage: (image) => {
    const { attachments } = get();
    if (attachments.length >= FUSE_MAX_IMAGES) return false;
    if (attachments.some((a) => a.mediaToken === image.mediaToken)) return true;
    set({ attachments: [...attachments, libraryRef(image)] });
    return true;
  },

  continueFrom: (result) => {
    const others = get().attachments.filter((a) => a.mediaToken !== result.mediaToken);
    set({ attachments: [libraryRef(result), ...others].slice(0, FUSE_MAX_IMAGES) });
  },

  startTurn: (turn) => set({ turns: [...get().turns, turn] }),

  completeTurn: (id, results) => {
    const turns = applyCompletion(get().turns, id, results);
    if (turns === get().turns) return false;
    set({ turns });
    return true;
  },

  failTurn: (id, reason) => {
    const turns = applyFailure(get().turns, id, reason);
    if (turns === get().turns) return false;
    set({ turns });
    return true;
  },

  clearConversation: () => set({ turns: [] }),
}));

// Returns the same array when no working turn has this id, so callers can
// tell events for other pages apart.
export function applyCompletion(turns: FuseTurn[], id: string, results: FuseResult[]): FuseTurn[] {
  const index = turns.findIndex((t) => t.id === id && t.status === "working");
  if (index < 0) return turns;
  const next = [...turns];
  next[index] = { ...turns[index], status: "done", results };
  return next;
}

export function applyFailure(turns: FuseTurn[], id: string, reason: string): FuseTurn[] {
  const index = turns.findIndex((t) => t.id === id && t.status === "working");
  if (index < 0) return turns;
  const next = [...turns];
  next[index] = { ...turns[index], status: "failed", maybeError: reason };
  return next;
}

// Library images need no upload; an empty File marks them like the image
// page's library picks do.
function libraryRef(image: FuseResult): RefImage {
  return {
    id: `fuse-${image.mediaToken}`,
    url: image.url,
    file: new File([], "library-image"),
    mediaToken: image.mediaToken,
  };
}
