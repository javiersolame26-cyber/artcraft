import { describe, expect, it } from "vitest";
import { findSharedImageUrl, parseWebAddress } from "./importFromWebAddress";

const PIN_URL = "https://www.pinterest.com/pin/123456789/";

describe("parseWebAddress", () => {
  it("accepts web links with or without a scheme", () => {
    expect(parseWebAddress(" https://i.pinimg.com/736x/ab/cd.jpg ")).toBe("https://i.pinimg.com/736x/ab/cd.jpg");
    expect(parseWebAddress("i.pinimg.com/736x/ab/cd.jpg")).toBe("https://i.pinimg.com/736x/ab/cd.jpg");
  });

  it("rejects everything else", () => {
    expect(parseWebAddress("")).toBeUndefined();
    expect(parseWebAddress("file:///C:/photo.png")).toBeUndefined();
    expect(parseWebAddress("not a url at all")).toBeUndefined();
  });
});

describe("findSharedImageUrl", () => {
  it("reads the page's preview image", () => {
    const html = `<html><head>
      <meta property="og:title" content="A pin">
      <meta property="og:image" content="https://i.pinimg.com/originals/ab/cd/ef.jpg">
    </head></html>`;
    expect(findSharedImageUrl(html, PIN_URL)).toBe("https://i.pinimg.com/originals/ab/cd/ef.jpg");
  });

  it("falls back to twitter:image and resolves relative links", () => {
    const html = `<head><meta name="twitter:image" content="/images/cover.png"></head>`;
    expect(findSharedImageUrl(html, "https://example.com/post/1")).toBe("https://example.com/images/cover.png");
  });

  it("finds nothing on pages without one", () => {
    expect(findSharedImageUrl("<html><body>hello</body></html>", PIN_URL)).toBeUndefined();
  });
});
