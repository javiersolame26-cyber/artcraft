import { useEffect, useMemo, useRef, useState } from "react";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { useSignals } from "@preact/signals-react/runtime";
import { arrayMove } from "@dnd-kit/sortable";
import { AlertTriangleIcon, ArrowUpIcon, BlendIcon, CornerDownRightIcon, LinkIcon, Loader2Icon, RotateCcwIcon, Trash2Icon } from "lucide-react";
import { twMerge } from "tailwind-merge";
import { UploadImageMedia, type UploadMediaFn } from "@storyteller/api";
import { GenerationProvider } from "@storyteller/api-enums";
import { CommonAspectRatio, LOCAL_KONTEXT_MODEL_ID } from "@storyteller/model-list";
import { ComfyUiTestConnection, GenerateImage, commandErrorMessage } from "@storyteller/tauri-api";
import { useTextToImageGenerationCompleteEvent } from "@storyteller/tauri-events";
import { Button } from "@storyteller/ui-button";
import {
  galleryModalLightboxImage,
  galleryModalLightboxMediaId,
  galleryModalLightboxNavNext,
  galleryModalLightboxNavPrev,
  galleryModalLightboxVisible,
} from "@storyteller/ui-gallery-modal";
import {
  DeckAddAction,
  DeckItem,
  PromptBoxDropOverlay,
  ReferenceDeck,
  useDeckMedia,
  usePromptBoxDrop,
} from "@storyteller/ui-promptbox";
import { toast } from "@storyteller/ui-toaster";
import { FUSE_MAX_IMAGES, FuseResult, FuseTurn, useFuseStore } from "./FuseStore";
import { fetchImageFromWebAddress, WebImageError } from "./importFromWebAddress";

const ASPECT_RATIOS: { value: CommonAspectRatio; label: string }[] = [
  { value: CommonAspectRatio.Auto, label: "Auto" },
  { value: CommonAspectRatio.Square, label: "1:1" },
  { value: CommonAspectRatio.WideThreeByTwo, label: "3:2" },
  { value: CommonAspectRatio.TallTwoByThree, label: "2:3" },
  { value: CommonAspectRatio.WideSixteenByNine, label: "16:9" },
  { value: CommonAspectRatio.TallNineBySixteen, label: "9:16" },
];

// Kontext reads its images side by side, so left/right phrasing works best.
const SUGGESTIONS_ONE_IMAGE = [
  "Change the background to a sunny beach, keep the subject the same",
  "Turn it into a watercolor painting",
  "Make it night time with soft neon lights",
];
const SUGGESTIONS_SEVERAL_IMAGES = [
  "Put the person on the left into the scene on the right, with matching light and shadows",
  "Apply the style of the right image to the left image",
  "Combine these images into one natural photo",
  "Dress the person on the left in the outfit on the right",
];

type ComfyStatus =
  | { kind: "checking" }
  | { kind: "ready" }
  | { kind: "missing"; files: string[] }
  | { kind: "offline"; message: string };

export const PageFuse = () => {
  useSignals();
  const attachments = useFuseStore((s) => s.attachments);
  const setAttachments = useFuseStore((s) => s.setAttachments);
  const instruction = useFuseStore((s) => s.instruction);
  const setInstruction = useFuseStore((s) => s.setInstruction);
  const aspectRatio = useFuseStore((s) => s.aspectRatio);
  const setAspectRatio = useFuseStore((s) => s.setAspectRatio);
  const turns = useFuseStore((s) => s.turns);
  const [sending, setSending] = useState(false);
  const [webAddressOpen, setWebAddressOpen] = useState(false);
  const comfyStatus = useComfyStatus();
  const scrollRef = useRef<HTMLDivElement>(null);

  useFuseGenerationEvents();

  const deck = useDeckMedia({
    referenceImages: attachments,
    setReferenceImages: setAttachments,
    maxImages: FUSE_MAX_IMAGES,
    uploadImage: UploadImageMedia as UploadMediaFn,
    ownGalleryModal: true,
  });

  const drop = usePromptBoxDrop({
    acceptsImages: true,
    acceptsVideos: false,
    acceptsAudio: false,
    onDropFiles: ({ images }) => addFiles(images),
  });

  const addFiles = (files: File[]) => {
    if (files.length === 0) return;
    if (deck.availableImageSlots <= 0) {
      toast.error(`Fuse takes up to ${FUSE_MAX_IMAGES} images`);
      return;
    }
    deck.processImageFiles(files, "start");
  };

  // Follow the conversation as it grows.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [turns.length, turns[turns.length - 1]?.status]);

  const deckItems: DeckItem[] = useMemo(
    () => [
      ...attachments.map((image, i) => ({
        id: image.id,
        kind: "image" as const,
        url: image.url,
        previewUrl: image.fullUrl ?? image.url,
        name: `Image ${i + 1}`,
      })),
      ...deck.uploadingImages.map((entry, i) => ({
        id: entry.id,
        kind: "image" as const,
        url: entry.previewUrl,
        name: `Image ${attachments.length + i + 1}`,
        uploading: true,
      })),
    ],
    [attachments, deck.uploadingImages],
  );

  const addActions: DeckAddAction[] = [
    { key: "upload", label: "Upload from this computer", group: "image", onSelect: deck.openImageUpload },
    { key: "library", label: "Pick from library", group: "image", onSelect: () => deck.openGallery("start") },
    { key: "web", label: "From a web address", icon: LinkIcon, group: "image", onSelect: () => setWebAddressOpen(true) },
  ];

  const uploading = deck.uploadingImages.length > 0;
  const canSend = instruction.trim().length > 0 && !uploading && !sending;

  const send = async () => {
    if (!canSend) return;
    const turn: FuseTurn = {
      id: newId(),
      instruction: instruction.trim(),
      inputs: attachments
        .filter((image) => image.mediaToken)
        .map((image) => ({ mediaToken: image.mediaToken, url: image.fullUrl ?? image.url })),
      aspectRatio,
      status: "working",
      results: [],
      createdAt: Date.now(),
    };
    const store = useFuseStore.getState();
    store.startTurn(turn);
    setInstruction("");
    setSending(true);
    try {
      await GenerateImage({
        model: LOCAL_KONTEXT_MODEL_ID,
        provider: GenerationProvider.ComfyUi,
        prompt: turn.instruction,
        aspect_ratio: turn.aspectRatio,
        batch_size: 1,
        image_media_tokens: turn.inputs.map((image) => image.mediaToken),
        frontend_caller: "text_to_image",
        frontend_subscriber_id: turn.id,
      });
      window.dispatchEvent(new Event("task-queue-update"));
    } catch (err) {
      store.failTurn(turn.id, commandErrorMessage(err, "Couldn't start the fusion. Is ComfyUI running?"));
    } finally {
      setSending(false);
    }
  };

  return (
    <div
      {...drop.dropZoneProps}
      className="fixed inset-0 flex flex-col bg-ui-background pt-[56px] text-base-fg"
    >
      <PromptBoxDropOverlay dragState={drop.dragState} acceptsImages acceptsVideos={false} acceptsAudio={false} />

      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 px-4 pb-8 pt-6 sm:px-6">
          <Header turnCount={turns.length} />
          <ComfyStatusBanner status={comfyStatus} />
          {turns.length === 0 ? (
            <EmptyState imageCount={attachments.length} onPick={setInstruction} />
          ) : (
            turns.map((turn) => <TurnView key={turn.id} turn={turn} />)
          )}
        </div>
      </div>

      <div className="border-t border-ui-panel-border bg-ui-panel/80 backdrop-blur">
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-4 py-4 sm:px-6">
          <div className="flex items-start gap-3">
            <ReferenceDeck
              items={deckItems}
              canAdd={deckItems.length < FUSE_MAX_IMAGES}
              addActions={addActions}
              addMenuGroupHints={{ image: `${attachments.length}/${FUSE_MAX_IMAGES}` }}
              onAddClick={deck.openImageUpload}
              onRemove={(id) => setAttachments(attachments.filter((image) => image.id !== id))}
              onReorderImages={(from, to) => setAttachments(arrayMove(attachments, from, to))}
              onClearAll={() => setAttachments([])}
              emptyLabel="Images"
            />
            <textarea
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  send();
                }
              }}
              rows={3}
              placeholder={
                attachments.length > 1
                  ? "Say how to fuse them, e.g. “put the person on the left into the scene on the right”"
                  : "Add images, then say what to make, e.g. “put this cat on a beach at sunset”"
              }
              className="min-h-[72px] flex-1 resize-none bg-transparent text-sm text-base-fg placeholder-base-fg/40 focus:outline-none"
            />
          </div>
          {webAddressOpen && (
            <WebAddressRow
              onCancel={() => setWebAddressOpen(false)}
              onImported={(file) => {
                addFiles([file]);
                setWebAddressOpen(false);
              }}
            />
          )}
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1">
              {ASPECT_RATIOS.map(({ value, label }) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => setAspectRatio(value)}
                  className={twMerge(
                    "rounded-[3px] border px-2 py-1 text-xs transition-colors",
                    aspectRatio === value
                      ? "border-primary bg-primary/20 text-base-fg"
                      : "border-ui-controls-border text-base-fg/60 hover:text-base-fg",
                  )}
                >
                  {label}
                </button>
              ))}
              <span className="ml-2 text-[11px] text-base-fg/40">
                Free on your GPU · Flux Kontext · English works best
              </span>
            </div>
            <Button
              variant="primary"
              icon={ArrowUpIcon}
              onClick={send}
              disabled={!canSend}
              loading={sending}
              className="px-4 py-1.5 text-sm"
            >
              Fuse
            </Button>
          </div>
        </div>
      </div>

      {deck.fileInputs}
      {deck.galleryModal}
    </div>
  );
};

const Header = ({ turnCount }: { turnCount: number }) => {
  const clear = useFuseStore((s) => s.clearConversation);
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <h1 className="flex items-center gap-2 font-display text-2xl tracking-tight">
          <BlendIcon className="h-6 w-6 text-orange-300" /> Fuse
        </h1>
        <p className="mt-1 max-w-xl text-sm text-base-fg/60">
          Bring images from the web or your library, then tell the assistant how
          to blend them. It runs on your own graphics card for free.
        </p>
      </div>
      {turnCount > 0 && (
        <Button variant="ghost" icon={Trash2Icon} onClick={clear} className="shrink-0 text-xs">
          New chat
        </Button>
      )}
    </div>
  );
};

const ComfyStatusBanner = ({ status }: { status: ComfyStatus }) => {
  if (status.kind === "checking" || status.kind === "ready") return null;
  const message =
    status.kind === "offline"
      ? `${status.message} Fuse runs on ComfyUI on this computer; start it and come back.`
      : `ComfyUI is running but Flux Kontext isn't installed yet (missing ${status.files.join(", ")}). Add the files, then check Settings → ComfyUI (Local).`;
  return (
    <div role="alert" className="flex gap-2 rounded-[3px] border border-amber-400/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
      <AlertTriangleIcon className="mt-0.5 h-4 w-4 shrink-0" />
      <p>{message}</p>
    </div>
  );
};

const EmptyState = ({ imageCount, onPick }: { imageCount: number; onPick: (text: string) => void }) => {
  const suggestions = imageCount > 1 ? SUGGESTIONS_SEVERAL_IMAGES : SUGGESTIONS_ONE_IMAGE;
  return (
    <div className="flex flex-col gap-4 rounded-[3px] border border-dashed border-ui-panel-border px-5 py-6">
      <ol className="flex flex-col gap-1.5 text-sm text-base-fg/70">
        <li>1. Add up to {FUSE_MAX_IMAGES} images with the + below: upload, drag in, paste (Ctrl+V), pick from your library, or paste a web address (a Pinterest pin works).</li>
        <li>2. Describe the result you want. The images are read side by side, left to right in their order.</li>
        <li>3. Keep talking: “Continue from this” edits the last result.</li>
      </ol>
      <div className="flex flex-wrap gap-2">
        {suggestions.map((text) => (
          <button
            key={text}
            type="button"
            onClick={() => onPick(text)}
            className="rounded-full border border-ui-controls-border bg-ui-controls/60 px-3 py-1.5 text-left text-xs text-base-fg/80 transition-colors hover:border-white/30 hover:text-base-fg"
          >
            {text}
          </button>
        ))}
      </div>
    </div>
  );
};

const TurnView = ({ turn }: { turn: FuseTurn }) => (
  <div className="flex flex-col gap-3">
    <div className="ml-auto flex max-w-[85%] flex-col items-end gap-2">
      {turn.inputs.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5">
          {turn.inputs.map((image, i) => (
            <div key={`${image.mediaToken}-${i}`} className="relative">
              <img src={image.url} alt={`Image ${i + 1}`} className="h-16 w-16 rounded-[3px] border border-ui-panel-border object-cover" />
              <span className="absolute left-1 top-1 rounded-sm bg-black/70 px-1 text-[10px] text-white">{i + 1}</span>
            </div>
          ))}
        </div>
      )}
      <p className="rounded-[3px] bg-primary/25 px-3 py-2 text-sm">{turn.instruction}</p>
    </div>
    <AssistantReply turn={turn} />
  </div>
);

const AssistantReply = ({ turn }: { turn: FuseTurn }) => {
  const continueFrom = useFuseStore((s) => s.continueFrom);
  const setInstruction = useFuseStore((s) => s.setInstruction);
  const setAttachments = useFuseStore((s) => s.setAttachments);

  if (turn.status === "working") {
    return (
      <div className="flex max-w-[85%] items-center gap-2 rounded-[3px] border border-ui-panel-border bg-ui-panel px-3 py-2 text-sm text-base-fg/70">
        <Loader2Icon className="h-4 w-4 animate-spin" />
        Fusing on your GPU… The first run loads the model and can take a couple of minutes.
      </div>
    );
  }

  if (turn.status === "failed") {
    return (
      <div className="flex max-w-[85%] flex-col gap-2 rounded-[3px] border border-red-400/40 bg-red-500/10 px-3 py-2 text-sm">
        <p className="text-red-300">{turn.maybeError ?? "The fusion failed."}</p>
        <div>
          <Button
            variant="secondary"
            icon={RotateCcwIcon}
            className="py-1 text-xs"
            onClick={() => {
              setAttachments(turn.inputs.map((image, i) => ({ id: `retry-${i}-${image.mediaToken}`, url: image.url, file: new File([], "library-image"), mediaToken: image.mediaToken })));
              setInstruction(turn.instruction);
            }}
          >
            Try again
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex max-w-[85%] flex-col gap-2 rounded-[3px] border border-ui-panel-border bg-ui-panel p-3">
      <p className="text-sm text-base-fg/80">Here it is. Tell me what to change and I'll keep working on it.</p>
      {turn.results.map((result) => (
        <div key={result.mediaToken} className="flex flex-col gap-2">
          <button type="button" onClick={() => openInLightbox(result, turn)} className="block overflow-hidden rounded-[3px]">
            <img src={result.url} alt={turn.instruction} className="max-h-[480px] w-auto max-w-full object-contain" />
          </button>
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" icon={CornerDownRightIcon} className="py-1 text-xs" onClick={() => continueFrom(result)}>
              Continue from this
            </Button>
            <Button variant="secondary" className="py-1 text-xs" onClick={() => openInLightbox(result, turn)}>
              Open · find similar online
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
};

const WebAddressRow = ({ onCancel, onImported }: { onCancel: () => void; onImported: (file: File) => void }) => {
  const [address, setAddress] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const importImage = async () => {
    setBusy(true);
    setError(null);
    try {
      onImported(await fetchImageFromWebAddress(address));
    } catch (err) {
      setError(err instanceof WebImageError ? err.message : "Couldn't download that image.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <div className="flex gap-2">
        <input
          autoFocus
          type="text"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") importImage();
            if (e.key === "Escape") onCancel();
          }}
          placeholder="Paste an image address or a Pinterest pin link"
          className="min-w-0 flex-1 rounded-[3px] border border-ui-controls-border bg-ui-controls px-3 py-1.5 text-sm outline-none focus:border-base-fg/40"
        />
        <Button variant="secondary" onClick={importImage} loading={busy} disabled={!address.trim() || busy} className="py-1.5 text-sm">
          Add
        </Button>
        <Button variant="ghost" onClick={onCancel} className="py-1.5 text-sm">
          Cancel
        </Button>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}
    </div>
  );
};

// Results and failures arrive as app-wide events; keep the ones for our turns.
const useFuseGenerationEvents = () => {
  useTextToImageGenerationCompleteEvent(async (event) => {
    const id = event.maybe_frontend_subscriber_id;
    if (!id) return;
    useFuseStore.getState().completeTurn(
      id,
      event.generated_images.map((image) => ({ mediaToken: image.media_token, url: image.cdn_url })),
    );
  });

  useEffect(() => {
    let unlisten: Promise<UnlistenFn> | undefined;
    try {
      unlisten = listen<{ data: { maybe_frontend_subscriber_id?: string; reason?: string } }>(
        "generation-failed-event",
        (event) => {
          const id = event.payload.data.maybe_frontend_subscriber_id;
          if (id) useFuseStore.getState().failTurn(id, event.payload.data.reason ?? "The fusion failed.");
        },
      );
    } catch {
      // Not running inside the desktop app.
    }
    return () => {
      unlisten?.then((stop) => stop()).catch(() => undefined);
    };
  }, []);
};

const useComfyStatus = (): ComfyStatus => {
  const [status, setStatus] = useState<ComfyStatus>({ kind: "checking" });
  useEffect(() => {
    ComfyUiTestConnection()
      .then((info) =>
        setStatus(info.missing_kontext_files.length > 0 ? { kind: "missing", files: info.missing_kontext_files } : { kind: "ready" }),
      )
      .catch((err) => setStatus({ kind: "offline", message: err instanceof Error ? err.message : String(err) }));
  }, []);
  return status;
};

const openInLightbox = (result: FuseResult, turn: FuseTurn) => {
  galleryModalLightboxNavPrev.value = null;
  galleryModalLightboxNavNext.value = null;
  galleryModalLightboxMediaId.value = result.mediaToken;
  galleryModalLightboxImage.value = {
    id: result.mediaToken,
    label: turn.instruction,
    thumbnail: result.url,
    fullImage: result.url,
    createdAt: new Date(turn.createdAt).toISOString(),
    mediaClass: "image",
  };
  galleryModalLightboxVisible.value = true;
};

const newId = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2));
