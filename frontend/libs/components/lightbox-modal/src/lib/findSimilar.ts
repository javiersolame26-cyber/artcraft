// Reverse image search in the user's browser. None of these services offers
// an open API for searching the whole web by image, but their websites accept
// a public image URL (Google Lens, Bing) or words (Pinterest).

export type SimilarSearchEngine = "google_lens" | "bing" | "pinterest";

export const SIMILAR_SEARCH_ENGINES: { engine: SimilarSearchEngine; label: string }[] = [
  { engine: "google_lens", label: "Google Lens" },
  { engine: "bing", label: "Bing" },
  { engine: "pinterest", label: "Pinterest" },
];

// Pinterest searches by words; long prompts match nothing, so keep the gist.
const MAX_PINTEREST_QUERY_LENGTH = 80;

// Returns undefined when the engine can't search with what we have
// (no public image URL for Lens/Bing, no words for Pinterest).
export function buildSimilarSearchUrl(
  engine: SimilarSearchEngine,
  imageUrl: string | undefined,
  query: string | undefined,
): string | undefined {
  switch (engine) {
    case "google_lens": {
      const publicUrl = publicImageUrl(imageUrl);
      return publicUrl ? `https://lens.google.com/uploadbyurl?url=${encodeURIComponent(publicUrl)}` : undefined;
    }
    case "bing": {
      const publicUrl = publicImageUrl(imageUrl);
      return publicUrl
        ? `https://www.bing.com/images/search?view=detailv2&iss=sbi&form=SBIVSP&sbisrc=UrlPaste&q=imgurl:${encodeURIComponent(publicUrl)}`
        : undefined;
    }
    case "pinterest": {
      const words = shortenQuery(query);
      return words ? `https://www.pinterest.com/search/pins/?q=${encodeURIComponent(words)}&rs=typed` : undefined;
    }
  }
}

// Only http(s) URLs are reachable by the search engines; blob:, data: and
// local file URLs live on this computer.
export function publicImageUrl(imageUrl: string | undefined): string | undefined {
  if (!imageUrl) return undefined;
  try {
    const url = new URL(imageUrl);
    if (url.protocol !== "https:" && url.protocol !== "http:") return undefined;
    if (url.hostname === "localhost" || url.hostname === "127.0.0.1") return undefined;
    return url.toString();
  } catch {
    return undefined;
  }
}

export function shortenQuery(query: string | undefined): string | undefined {
  const words = (query ?? "").replace(/\s+/g, " ").trim();
  if (!words) return undefined;
  if (words.length <= MAX_PINTEREST_QUERY_LENGTH) return words;
  const cut = words.slice(0, MAX_PINTEREST_QUERY_LENGTH);
  const lastSpace = cut.lastIndexOf(" ");
  return (lastSpace > 20 ? cut.slice(0, lastSpace) : cut).replace(/[,.;:]+$/, "");
}
