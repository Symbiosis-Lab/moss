import { describe, test, expect, beforeEach } from "vitest";
import { pictureImg, sectionPictures, thumbnailUrl } from "../nav/section-pictures";

const pic = (name: string) =>
  `<picture><source srcset="${name}-1600.webp 1600w, ${name}-800.webp 800w, ${name}.webp 2400w" type="image/webp" sizes="100vw">` +
  `<img src="${name}.jpg" alt=""></picture>`;

describe("section pictures", () => {
  beforeEach(() => {
    document.body.innerHTML = `<main>
      <h1>Title</h1>${pic("cover")}
      <h2 id="a">A</h2><p>no image</p>
      <h2 id="b">B</h2><h3 id="b1">B1</h3>${pic("deep")}
      <h2 id="c">C</h2><img src="plain.svg" alt="" style="object-position: 20% 70%">${pic("second")}
    </main>`;
  });

  test("the narrowest generated width is used, not the page's own src", () => {
    const img = document.querySelector<HTMLImageElement>("picture img")!;
    expect(thumbnailUrl(img)).toBe("cover-800.webp");
  });

  test("an image with no ladder uses its only file", () => {
    expect(thumbnailUrl(document.querySelector<HTMLImageElement>('img[src="plain.svg"]')!)).toBe("plain.svg");
  });

  test("a section owns the first image inside it, subsections included; one before the first section belongs to none", () => {
    const heads = [...document.querySelectorAll<HTMLElement>("h2")];
    expect(sectionPictures(heads).map((p) => p?.src ?? null)).toEqual([null, "deep-800.webp", "plain.svg"]);
  });

  test("a subsection stops at the next heading of its own or a higher level", () => {
    const heads = [...document.querySelectorAll<HTMLElement>("h3")];
    expect(sectionPictures(heads).map((p) => p?.src)).toEqual(["deep-800.webp"]);
  });

  test("the thumbnail keeps the page image's focal point", () => {
    const heads = [...document.querySelectorAll<HTMLElement>("h2")];
    const picture = sectionPictures(heads)[2]!;
    expect(picture.position).toBe("20% 70%");
    expect(pictureImg(picture, () => {}).style.objectPosition).toBe("20% 70%");
  });
});
