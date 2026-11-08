// File viewer for non-text files (images, video, audio, PDF, fonts, binary).
// Renders inside the #moss-editor-cm container, replacing CodeMirror.

import type { FileCategory } from './file-types';
import { sourceAssetUrl } from './source-asset-url';
import { toRelative } from './path-domain';
import { t } from "../i18n/index";
import { engine } from '../engine';

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

function createViewer(): HTMLElement {
  const el = document.createElement('div');
  el.className = 'moss-file-viewer';
  return el;
}

async function renderImage(container: HTMLElement, filePath: string, projectRoot: string): Promise<void> {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content moss-file-viewer__checkerboard';

  const img = document.createElement('img');
  img.src = sourceAssetUrl(toRelative(filePath, projectRoot));
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

  // Info bar with metadata
  const filename = filePath.split('/').pop() ?? '';
  const result = await engine.getFileInfo(filePath);
  if (result.status === 'ok') {
    const info = result.data;
    const dims = info.width && info.height ? `${info.width}\u00D7${info.height}` : '';
    viewer.appendChild(createInfoBar([filename, dims, formatFileSize(info.size)]));
  } else {
    viewer.appendChild(createInfoBar([filename]));
  }

  container.appendChild(viewer);
  currentViewerEl = viewer;
}

function renderVideo(container: HTMLElement, filePath: string, projectRoot: string): void {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content';

  const video = document.createElement('video');
  video.src = sourceAssetUrl(toRelative(filePath, projectRoot));
  video.controls = true;
  video.style.maxWidth = '100%';

  content.appendChild(video);
  viewer.appendChild(content);

  const filename = filePath.split('/').pop() ?? '';
  engine.getFileInfo(filePath).then((result) => {
    if (result.status === 'ok') {
      viewer.appendChild(createInfoBar([filename, formatFileSize(result.data.size)]));
    } else {
      viewer.appendChild(createInfoBar([filename]));
    }
  });

  container.appendChild(viewer);
  currentViewerEl = viewer;
}

function renderAudio(container: HTMLElement, filePath: string, projectRoot: string): void {
  const viewer = createViewer();
  const content = document.createElement('div');
  content.className = 'moss-file-viewer__content';

  const audio = document.createElement('audio');
  audio.src = sourceAssetUrl(toRelative(filePath, projectRoot));
  audio.controls = true;

  content.appendChild(audio);
  viewer.appendChild(content);

  const filename = filePath.split('/').pop() ?? '';
  engine.getFileInfo(filePath).then((result) => {
    if (result.status === 'ok') {
      viewer.appendChild(createInfoBar([filename, formatFileSize(result.data.size)]));
    } else {
      viewer.appendChild(createInfoBar([filename]));
    }
  });

  container.appendChild(viewer);
  currentViewerEl = viewer;
}

function renderPdf(container: HTMLElement, filePath: string, projectRoot: string): void {
  const viewer = createViewer();

  const embed = document.createElement('embed');
  embed.src = sourceAssetUrl(toRelative(filePath, projectRoot));
  embed.type = 'application/pdf';
  embed.className = 'moss-file-viewer__pdf';

  viewer.appendChild(embed);

  const filename = filePath.split('/').pop() ?? '';
  engine.getFileInfo(filePath).then((result) => {
    if (result.status === 'ok') {
      viewer.appendChild(createInfoBar([filename, formatFileSize(result.data.size)]));
    } else {
      viewer.appendChild(createInfoBar([filename]));
    }
  });

  container.appendChild(viewer);
  currentViewerEl = viewer;
}

function renderFont(container: HTMLElement, filePath: string, projectRoot: string): void {
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

  const filename = filePath.split('/').pop() ?? '';
  engine.getFileInfo(filePath).then((result) => {
    if (result.status === 'ok') {
      viewer.appendChild(createInfoBar([filename, formatFileSize(result.data.size)]));
    } else {
      viewer.appendChild(createInfoBar([filename]));
    }
  });

  container.appendChild(viewer);
  currentViewerEl = viewer;
}

function renderBinary(container: HTMLElement, filePath: string): void {
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
  container.appendChild(viewer);
  currentViewerEl = viewer;
}

export function showFileViewer(container: HTMLElement, filePath: string, projectRoot: string, category: FileCategory): void {
  destroyFileViewer();
  container.innerHTML = '';

  switch (category) {
    case 'image':
      renderImage(container, filePath, projectRoot);
      break;
    case 'video':
      renderVideo(container, filePath, projectRoot);
      break;
    case 'audio':
      renderAudio(container, filePath, projectRoot);
      break;
    case 'pdf':
      renderPdf(container, filePath, projectRoot);
      break;
    case 'font':
      renderFont(container, filePath, projectRoot);
      break;
    default:
      renderBinary(container, filePath);
      break;
  }
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
