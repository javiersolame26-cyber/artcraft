import { FetchProxy } from "@storyteller/tauri-utils";

// Bring an image into Fuse from a web address the user pasted: either a direct
// image link ("Copy image address") or a page that shares one, such as a
// Pinterest pin. Pages are read the way chat apps build link previews, from
// their og:image / twitter:image tag. One request per paste, nothing more.

const MAX_IMAGE_BYTES = 25 * 1024 * 1024;

export class WebImageError extends Error {}

export async function fetchImageFromWebAddress(address: string): Promise<File> {
  const url = parseWebAddress(address);
  if (!url) throw new WebImageError("That doesn't look like a web address.");

  const response = await get(url);
  const contentType = response.headers.get("content-type") ?? "";

  if (contentType.startsWith("image/")) {
    return toFile(await response.blob(), url, contentType);
  }

  if (contentType.includes("text/html")) {
    const shared = findSharedImageUrl(await response.text(), url);
    if (!shared) throw new WebImageError("That page doesn't share an image. Open the image itself and copy its address.");
    const imageResponse = await get(shared);
    const imageType = imageResponse.headers.get("content-type") ?? "";
    if (!imageType.startsWith("image/")) throw new WebImageError("The page's image couldn't be downloaded.");
    return toFile(await imageResponse.blob(), shared, imageType);
  }

  throw new WebImageError("That address isn't an image.");
}

export function parseWebAddress(address: string): string | undefined {
  const trimmed = address.trim();
  if (!trimmed) return undefined;
  try {
    const url = new URL(/^[a-z]+:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`);
    if (url.protocol !== "https:" && url.protocol !== "http:") return undefined;
    return url.toString();
  } catch {
    return undefined;
  }
}

// The image a page offers for link previews, resolved against the page URL.
export function findSharedImageUrl(html: string, pageUrl: string): string | undefined {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const selectors = [
    'meta[property="og:image:secure_url"]',
    'meta[property="og:image"]',
    'meta[name="og:image"]',
    'meta[name="twitter:image"]',
    'meta[property="twitter:image"]',
    'link[rel="image_src"]',
  ];
  for (const selector of selectors) {
    const element = doc.querySelector(selector);
    const value = element?.getAttribute("content") ?? element?.getAttribute("href");
    if (value) {
      try {
        return new URL(value, pageUrl).toString();
      } catch {
        // Try the next tag.
      }
    }
  }
  return undefined;
}

async function get(url: string): Promise<Response> {
  let response: Response;
  try {
    response = await FetchProxy(url, { method: "GET" });
  } catch {
    throw new WebImageError("Couldn't reach that address. Check your connection.");
  }
  if (!response.ok) throw new WebImageError(`That address answered with an error (${response.status}).`);
  return response;
}

function toFile(blob: Blob, url: string, contentType: string): File {
  if (blob.size === 0) throw new WebImageError("The image is empty.");
  if (blob.size > MAX_IMAGE_BYTES) throw new WebImageError("The image is larger than 25 MB.");
  return new File([blob], fileNameFor(url, contentType), { type: contentType.split(";")[0] });
}

function fileNameFor(url: string, contentType: string): string {
  const extension = contentType.split(";")[0].split("/")[1]?.replace("jpeg", "jpg") || "png";
  const last = new URL(url).pathname.split("/").pop() ?? "";
  const base = last.replace(/\.[a-z0-9]+$/i, "").replace(/[^a-z0-9_-]/gi, "").slice(0, 40);
  return `${base || "web-image"}.${extension}`;
}
