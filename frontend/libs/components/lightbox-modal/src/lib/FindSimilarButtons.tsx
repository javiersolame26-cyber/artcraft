import { useEffect, useState } from "react";
import { Button } from "@storyteller/ui-button";
import { OpenUrl } from "@storyteller/tauri-api";
import { toast } from "@storyteller/ui-toaster";
import { gtagEvent } from "@storyteller/google-analytics";
import { SearchIcon } from "lucide-react";
import {
  buildSimilarSearchUrl,
  publicImageUrl,
  shortenQuery,
  SimilarSearchEngine,
} from "./findSimilar";

interface FindSimilarButtonsProps {
  imageUrl?: string;
  // The generation prompt, used as the starting words for Pinterest.
  prompt?: string | null;
}

// "Find similar online": opens the image in Google Lens or Bing visual search,
// or a Pinterest search, in the user's browser.
export function FindSimilarButtons({ imageUrl, prompt }: FindSimilarButtonsProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState(shortenQuery(prompt ?? undefined) ?? "");
  const hasPublicUrl = !!publicImageUrl(imageUrl);

  useEffect(() => {
    setQuery(shortenQuery(prompt ?? undefined) ?? "");
  }, [prompt]);

  const search = async (engine: SimilarSearchEngine) => {
    const url = buildSimilarSearchUrl(engine, imageUrl, query);
    if (!url) return;
    gtagEvent("find_similar_clicked", { engine });
    try {
      await OpenUrl(url);
    } catch {
      // Outside the desktop app (e.g. the web build) fall back to a new tab.
      if (!window.open(url, "_blank", "noopener")) {
        toast.error("Couldn't open your browser");
      }
    }
  };

  return (
    <>
      <Button
        className="col-span-2 w-full py-1.5 text-[13px]"
        variant="secondary"
        icon={SearchIcon}
        aria-expanded={open}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((current) => !current);
        }}
      >
        Find similar online
      </Button>
      {open && (
        <div
          className="col-span-2 flex flex-col gap-2 rounded-[3px] border border-white/10 bg-black/20 p-2"
          onClick={(e) => e.stopPropagation()}
        >
          <p className="text-[11px] leading-snug text-white/60">
            Opens in your browser. Google Lens and Bing search the whole web for
            this image, Pinterest pins included. Pinterest itself searches by words.
          </p>
          <div className="grid grid-cols-2 gap-2">
            <Button
              className="w-full py-1.5 text-[13px]"
              variant="secondary"
              disabled={!hasPublicUrl}
              onClick={() => search("google_lens")}
            >
              Google Lens
            </Button>
            <Button
              className="w-full py-1.5 text-[13px]"
              variant="secondary"
              disabled={!hasPublicUrl}
              onClick={() => search("bing")}
            >
              Bing
            </Button>
          </div>
          {!hasPublicUrl && (
            <p className="text-[11px] text-amber-400">
              This image isn't online yet, so only the Pinterest word search works.
            </p>
          )}
          <div className="flex gap-2">
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") search("pinterest");
              }}
              placeholder="Words to search on Pinterest"
              aria-label="Words to search on Pinterest"
              className="min-w-0 flex-1 rounded-[3px] border border-white/15 bg-white/5 px-2 py-1.5 text-[13px] text-white outline-none placeholder:text-white/30 focus:border-white/40"
            />
            <Button
              className="py-1.5 text-[13px]"
              variant="secondary"
              disabled={!query.trim()}
              onClick={() => search("pinterest")}
            >
              Pinterest
            </Button>
          </div>
        </div>
      )}
    </>
  );
}
