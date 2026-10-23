import { describe, expect, it } from "vitest";
import { buildSimilarSearchUrl, publicImageUrl, shortenQuery } from "./findSimilar";

const IMAGE_URL = "https://cdn-2.fakeyou.com/media/a/b/image_123.png";

describe("buildSimilarSearchUrl", () => {
  it("sends the image URL to Google Lens", () => {
    expect(buildSimilarSearchUrl("google_lens", IMAGE_URL, undefined)).toBe(
      "https://lens.google.com/uploadbyurl?url=https%3A%2F%2Fcdn-2.fakeyou.com%2Fmedia%2Fa%2Fb%2Fimage_123.png",
    );
  });

  it("sends the image URL to Bing visual search", () => {
    const url = buildSimilarSearchUrl("bing", IMAGE_URL, undefined)!;
    expect(url.startsWith("https://www.bing.com/images/search?")).toBe(true);
    expect(url).toContain("q=imgurl:https%3A%2F%2Fcdn-2.fakeyou.com");
  });

  it("searches Pinterest by words", () => {
    expect(buildSimilarSearchUrl("pinterest", IMAGE_URL, "red fox, snowy forest")).toBe(
      "https://www.pinterest.com/search/pins/?q=red%20fox%2C%20snowy%20forest&rs=typed",
    );
  });

  it("can't search without what each engine needs", () => {
    expect(buildSimilarSearchUrl("google_lens", "blob:http://localhost/123", "fox")).toBeUndefined();
    expect(buildSimilarSearchUrl("bing", undefined, "fox")).toBeUndefined();
    expect(buildSimilarSearchUrl("pinterest", IMAGE_URL, "   ")).toBeUndefined();
  });
});

describe("publicImageUrl", () => {
  it("rejects local and non-web URLs", () => {
    expect(publicImageUrl("file:///C:/Users/me/photo.png")).toBeUndefined();
    expect(publicImageUrl("data:image/png;base64,AAAA")).toBeUndefined();
    expect(publicImageUrl("http://127.0.0.1:8188/view?filename=x.png")).toBeUndefined();
    expect(publicImageUrl("not a url")).toBeUndefined();
  });
});

describe("shortenQuery", () => {
  it("keeps short prompts and trims long ones at a word boundary", () => {
    expect(shortenQuery("  a  red fox ")).toBe("a red fox");
    const long = "a red fox sitting in a snowy forest at sunrise, photorealistic, cinematic lighting, 85mm lens, award winning";
    const short = shortenQuery(long)!;
    expect(short.length).toBeLessThanOrEqual(80);
    expect(long.startsWith(short)).toBe(true);
    expect(short.endsWith(",")).toBe(false);
  });
});
