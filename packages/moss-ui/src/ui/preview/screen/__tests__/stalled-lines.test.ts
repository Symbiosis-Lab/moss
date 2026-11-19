// The stalled screen names the provider that is not sending and the one
// thing to do about it; it offers quitting only for providers that are apps.
import { describe, it, expect, afterEach } from "vitest";
import { setLocale } from "../../../i18n";
import { stalledLines } from "../cloud-wait";

describe("stalledLines", () => {
  afterEach(() => setLocale("en"));

  it("says Google Drive is not sending files, and to quit and reopen it", () => {
    expect(stalledLines({ id: "google_drive", label: "Google Drive" })).toEqual({
      title: "Google Drive isn't sending files.",
      hint: "Quit Google Drive and open it again.",
    });
  });

  it("offers no quit for iCloud Drive, which is part of the system", () => {
    expect(stalledLines({ id: "icloud", label: "iCloud" })).toEqual({
      title: "iCloud Drive isn't sending files.",
      hint: "",
    });
  });

  it("does not call an unidentified provider by a placeholder name", () => {
    expect(stalledLines({ id: "unknown", label: "cloud" })).toEqual({ title: "Files aren't arriving.", hint: "" });
    expect(stalledLines(undefined)).toEqual({ title: "Files aren't arriving.", hint: "" });
  });

  it("names the provider in the reader's language", () => {
    setLocale("zh-hans");
    expect(stalledLines({ id: "google_drive", label: "Google Drive" })).toEqual({
      title: "Google 云端硬盘 没有发送文件。",
      hint: "退出 Google 云端硬盘，然后重新打开。",
    });
  });
});
