// File viewer for non-text files (images, video, audio, PDF, fonts, binary).
// Renders inside the #moss-editor-cm container, replacing CodeMirror.

import type { FileCategory } from './file-types';
import { sourceAssetUrl } from './source-asset-url';
import { toRelative } from './path-domain';
import { t } from "../i18n/index";
import { engine } from '../engine';
import type { EditorFileInfo } from '@symbiosis-lab/moss-ui';

let currentViewerEl: HTMLElement | null = null;
let currentFontStyleEl: HTMLStyleElement | null = null;

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function createInfoBar(items: string[]): HTMLElement {
  const bar = document.createElement('div');
  bar.className = 'moss-file-viewer__info';
  bar.textContent = items.filter(Boolean).join(' \u00B7 ');
  return bar;
}

/** The file's name, then what `describe` reads from its info, under the viewer once the info is in. */
function appendInfoBar(viewer: HTMLElement, filePath: string, describe: (info: EditorFileInfo) => string[] = (info) => [formatFileSize(info.size)]): void {
  const filename = filePath.split('/').pop() ?? '';
  void engine.getFileInfo(filePath).then((result) => {
    viewer.appendChild(createInfoBar([filename, ...(result.status === 'ok' ? describe(result.data) : [])]));
  });
}

function createViewer(): HTMLElement {
  const el = document.createElement('div');
  el.className = 'moss-file-viewer';
  return el;
}

function renderImage(filePath: string, projectRoot: string): HTMLElement {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content moss-file-viewer__checkerboard';

  const img = document.createElement('img');
  // The file itself, not a built rung: this view zooms to full size.
  img.src = sourceAssetUrl(toRelative(filePath, projectRoot), '?original');
  img.alt = filePath.split('/').pop() ?? '';
  img.draggable = false;

  let zoomed = false;
  img.addEventListener('click', () => {
    zoomed = !zoomed;
    content.classList.toggle('moss-file-viewer__zoom-active', zoomed);
    if (zoomed) {
      img.style.maxWidth = 'none';
      img.style.maxHeight = 'none';
    } else {
      img.style.maxWidth = '';
      img.style.maxHeight = '';
    }
  });

  content.appendChild(img);
  viewer.appendChild(content);

  appendInfoBar(viewer, filePath, (info) => [
    info.width && info.height ? `${info.width}\u00D7${info.height}` : '',
    formatFileSize(info.size),
  ]);
  return viewer;
}

function renderVideo(filePath: string, projectRoot: string): HTMLElement {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content';

  const video = document.createElement('video');
  video.src = sourceAssetUrl(toRelative(filePath, projectRoot));
  video.controls = true;
  video.style.maxWidth = '100%';

  content.appendChild(video);
  viewer.appendChild(content);

  appendInfoBar(viewer, filePath);
  return viewer;
}

function renderAudio(filePath: string, projectRoot: string): HTMLElement {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content';

  const audio = document.createElement('audio');
  audio.src = sourceAssetUrl(toRelative(filePath, projectRoot));
  audio.controls = true;

  content.appendChild(audio);
  viewer.appendChild(content);

  appendInfoBar(viewer, filePath);
  return viewer;
}

function renderPdf(filePath: string, projectRoot: string): HTMLElement {
  const viewer = createViewer();

  const embed = document.createElement('embed');
  embed.src = sourceAssetUrl(toRelative(filePath, projectRoot));
  embed.type = 'application/pdf';
  embed.className = 'moss-file-viewer__pdf';

  viewer.appendChild(embed);

  appendInfoBar(viewer, filePath);
  return viewer;
}

function renderFont(filePath: string, projectRoot: string): HTMLElement {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content moss-file-viewer__font-samples';

  const fontFamily = `moss-preview-font-${Date.now()}`;
  const fontUrl = sourceAssetUrl(toRelative(filePath, projectRoot));

  // Inject @font-face (escape URL to prevent CSS injection from filenames with special chars)
  const style = document.createElement('style');
  const safeFontUrl = fontUrl.replace(/["\\]/g, '\\$&');
  style.textContent = `@font-face { font-family: "${fontFamily}"; src: url("${safeFontUrl}"); }`;
  document.head.appendChild(style);
  currentFontStyleEl = style;

  const sampleText = 'The quick brown fox jumps over the lazy dog';
  const sizes = [12, 18, 24, 36, 48, 72];

  for (const size of sizes) {
    const line = document.createElement('div');
    line.className = 'moss-file-viewer__font-line';
    const label = document.createElement('span');
    label.className = 'moss-file-viewer__font-label';
    label.textContent = `${size}px`;
    const sample = document.createElement('span');
    sample.style.fontFamily = `"${fontFamily}"`;
    sample.style.fontSize = `${size}px`;
    sample.textContent = sampleText;
    line.appendChild(label);
    line.appendChild(sample);
    content.appendChild(line);
  }

  // Full character set
  const charSet = document.createElement('div');
  charSet.className = 'moss-file-viewer__font-line';
  charSet.style.fontFamily = `"${fontFamily}"`;
  charSet.style.fontSize = '24px';
  charSet.textContent = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ\nabcdefghijklmnopqrstuvwxyz\n0123456789\n!@#$%^&*()_+-=[]{}|;:\'",.<>?/\\~`';
  content.appendChild(charSet);

  viewer.appendChild(content);

  appendInfoBar(viewer, filePath);
  return viewer;
}

function renderBinary(filePath: string): HTMLElement {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content';

  const card = document.createElement('div');
  card.className = 'moss-file-viewer__binary-card';

  const icon = document.createElement('div');
  icon.className = 'moss-file-viewer__binary-icon';
  icon.textContent = '\uD83D\uDCC4'; // file emoji

  const filename = filePath.split('/').pop() ?? '';
  const ext = filePath.split('.').pop()?.toUpperCase() ?? '';

  const name = document.createElement('div');
  name.className = 'moss-file-viewer__binary-name';
  name.textContent = filename;

  const msg = document.createElement('div');
  msg.className = 'moss-file-viewer__binary-msg';
  msg.textContent = t('editmisc.cannot_preview', {
    ext: ext ? ` (.${ext.toLowerCase()})` : '',
  });

  card.appendChild(icon);
  card.appendChild(name);

  engine.getFileInfo(filePath).then((result) => {
    if (result.status === 'ok') {
      const size = document.createElement('div');
      size.className = 'moss-file-viewer__binary-size';
      size.textContent = formatFileSize(result.data.size);
      card.appendChild(size);
    }
  });

  card.appendChild(msg);
  content.appendChild(card);
  viewer.appendChild(content);
  return viewer;
}

function renderViewer(filePath: string, projectRoot: string, category: FileCategory): HTMLElement {
  switch (category) {
    case 'image': return renderImage(filePath, projectRoot);
    case 'video': return renderVideo(filePath, projectRoot);
    case 'audio': return renderAudio(filePath, projectRoot);
    case 'pdf': return renderPdf(filePath, projectRoot);
    case 'font': return renderFont(filePath, projectRoot);
    default: return renderBinary(filePath);
  }
}

/** The viewer is on screen and counted open when this returns; only its info bar arrives later. */
export function showFileViewer(container: HTMLElement, filePath: string, projectRoot: string, category: FileCategory): void {
  destroyFileViewer();
  container.innerHTML = '';
  currentViewerEl = container.appendChild(renderViewer(filePath, projectRoot, category));
}

export function fileViewerIsOpen(): boolean {
  return currentViewerEl !== null;
}

export function destroyFileViewer(): void {
  if (currentViewerEl) {
    currentViewerEl.remove();
    currentViewerEl = null;
  }
  if (currentFontStyleEl) {
    currentFontStyleEl.remove();
    currentFontStyleEl = null;
  }
}
