import { t } from "../../i18n/index";
import type { NoticeVM } from "./progress-panel-stage";

type CloudFiles = NonNullable<NoticeVM["cloudFiles"]>;

/** One native disclosure; its open state and file nodes survive rail renders. */
export class CloudFileDisclosure {
  private open = false;
  private listCache: { files: string[]; element: HTMLUListElement } | null = null;
  private onRequest: (() => void) | null = null;

  setCallback(onRequest: () => void): void {
    this.onRequest = onRequest;
  }

  isOpen(): boolean {
    return this.open;
  }

  reset(): void {
    this.open = false;
    this.listCache = null;
  }

  render({ count, status, files }: CloudFiles): HTMLDetailsElement {
    const detail = document.createElement("details");
    detail.className = "moss-notice-cloud-files";
    detail.open = this.open;
    const summary = document.createElement("summary");
    summary.className = "moss-notice-cloud-files-toggle";
    summary.textContent = count === null
      ? t('preview.cloud_unavailable_show_files')
      : t('preview.cloud_unavailable_show_all', { count });
    detail.appendChild(summary);

    const content = document.createElement("div");
    content.className = "moss-notice-cloud-files-content";
    const statusText = document.createElement("span");
    statusText.setAttribute("aria-live", "polite");
    if (status === "loading" && files.length > 0) statusText.textContent = t('preview.cloud_unavailable_checking');
    else if (status === "loading" || status === "idle") statusText.textContent = t('preview.cloud_unavailable_loading');
    else if (status === "error") statusText.textContent = t('preview.cloud_unavailable_list_error');
    else if (files.length === 0) statusText.textContent = t('preview.cloud_unavailable_none');
    if (statusText.textContent) content.appendChild(statusText);

    if (files.length > 0) {
      if (!this.listCache || this.listCache.files.length !== files.length ||
          this.listCache.files.some((file, index) => file !== files[index])) {
        const list = document.createElement("ul");
        for (const file of files) {
          const item = document.createElement("li");
          item.textContent = file;
          list.appendChild(item);
        }
        this.listCache = { files, element: list };
      }
      content.appendChild(this.listCache.element);
    }
    if (status === "error" || status === "loaded") {
      const refresh = document.createElement("button");
      refresh.type = "button";
      refresh.className = "moss-notice-cloud-files-refresh";
      refresh.textContent = t(status === "error"
        ? 'preview.cloud_unavailable_retry' : 'preview.cloud_unavailable_refresh');
      refresh.addEventListener("click", () => this.onRequest?.());
      content.appendChild(refresh);
    }
    detail.appendChild(content);
    detail.addEventListener("toggle", () => {
      if (!detail.isConnected) return;
      this.open = detail.open;
      if (detail.open && status === "idle") this.onRequest?.();
    });
    return detail;
  }
}
