const COPY = {
  en: {
    play: 'Play', pause: 'Stop', replay: 'Replay',
    ready: 'Ready · press Play', typing: 'Writing Markdown', paused: 'Paused · explore the editor or resume',
    done: 'Done · the page is still yours to explore', explored: 'Your edits are preserved · Replay starts over', reduced: 'Static example · motion is reduced',
    error: 'The interactive editor could not load',
    frame: 'Interactive moss editor demonstration',
    sample: '## A small heading\n\nA plain paragraph, with **bold** or *italic* text.\n\n- A list item\n- Another item\n\n> A quote.\n\n[A link](https://example.com)',
  },
  'zh-hant': {
    play: '播放', pause: '停止', replay: '重播',
    ready: '準備完成 · 按下播放', typing: '正在寫入 Markdown', paused: '已暫停 · 可自由操作或繼續播放',
    done: '完成 · 仍可自由操作編輯器', explored: '已保留你的改動 · 重播會重新開始', reduced: '靜態範例 · 已減少動態效果',
    error: '無法載入互動編輯器',
    frame: '青苔編輯器互動示範',
    sample: '## 小標題\n\n一段普通文字，可以**加粗**或*傾斜*。\n\n- 列表的一項\n- 另一項\n\n> 一段引用。\n\n[一個連結](https://example.com)',
  },
  'zh-hans': {
    play: '播放', pause: '停止', replay: '重播',
    ready: '准备完成 · 点击播放', typing: '正在写入 Markdown', paused: '已暂停 · 可自由操作或继续播放',
    done: '完成 · 仍可自由操作编辑器', explored: '已保留你的改动 · 重播会重新开始', reduced: '静态示例 · 已减少动态效果',
    error: '无法加载交互编辑器',
    frame: '青苔编辑器交互演示',
    sample: '## 小标题\n\n一段普通文字，可以**加粗**或*倾斜*。\n\n- 列表的一项\n- 另一项\n\n> 一段引用。\n\n[一个链接](https://example.com)',
  },
};

const MODULE_BASE = new URL('.', import.meta.url);

if (!document.querySelector('link[data-moss-ui-demo]')) {
  const stylesheet = document.createElement('link');
  stylesheet.rel = 'stylesheet';
  stylesheet.href = new URL('moss-ui-demo.css', MODULE_BASE).href;
  stylesheet.dataset.mossUiDemo = '';
  document.head.appendChild(stylesheet);
}

class MossUiDemo extends HTMLElement {
  constructor() {
    super();
    this.state = 'loading';
    this.run = 0;
    this.readyTimer = 0;
    this.readyAttempts = 0;
    this.reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  }

  connectedCallback() {
    const lang = this.getAttribute('lang') || document.documentElement.lang || 'en';
    this.copy = COPY[lang] || COPY.en;
    this.sample = this.copy.sample;
    this.static = this.hasAttribute('static');
    this.innerHTML = `
      ${this.static ? '' : `<div class="moss-ui-demo__bar">
        <p class="moss-ui-demo__status" role="status" aria-live="polite">${this.copy.ready}</p>
        <button type="button" data-action="play" disabled>${this.copy.play}</button>
      </div>`}
      <div class="moss-ui-demo__stage">
        <iframe title="${this.copy.frame}" src="${new URL('editor.html?measure=560&root=William%20Blake&file=Notebook.md&date=1794-07-01&doc=', MODULE_BASE)}" loading="lazy"></iframe>
      </div>`;
    this.frame = this.querySelector('iframe');
    this.playButton = this.querySelector('[data-action="play"]');
    this.status = this.querySelector('.moss-ui-demo__status');
    this.playButton?.addEventListener('click', this.onPlay);
    this.frame.addEventListener('load', this.onFrameLoad);
  }

  disconnectedCallback() {
    this.cancel();
    this.frame?.removeEventListener('load', this.onFrameLoad);
    this.playButton?.removeEventListener('click', this.onPlay);
    this.unbindExploration();
  }

  onFrameLoad = () => this.waitForEditor();

  waitForEditor() {
    window.clearTimeout(this.readyTimer);
    const editor = this.frame?.contentWindow?.__editor;
    if (!editor || this.frame.contentDocument?.documentElement.dataset.ready !== '1') {
      if (++this.readyAttempts >= 250) {
        this.setState('error');
        return;
      }
      this.readyTimer = window.setTimeout(() => this.waitForEditor(), 40);
      return;
    }
    this.editor = editor;
    if (this.static) {
      this.editor.setDoc(this.sample);
      this.setState('ready');
      return;
    }
    this.bindExploration();
    this.playButton.disabled = false;
    if (this.reduced) {
      this.editor.setDoc(this.sample);
      this.setState('reduced');
    } else {
      this.setState('ready');
    }
  }

  bindExploration() {
    this.unbindExploration();
    this.frameDocument = this.frame.contentDocument;
    this.frameDocument?.addEventListener('pointerdown', this.onExplore, true);
    this.frameDocument?.addEventListener('keydown', this.onExplore, true);
  }

  unbindExploration() {
    this.frameDocument?.removeEventListener('pointerdown', this.onExplore, true);
    this.frameDocument?.removeEventListener('keydown', this.onExplore, true);
    this.frameDocument = null;
  }

  onExplore = () => {
    if (this.state === 'typing') this.pause();
    queueMicrotask(() => {
      if (this.editor && !this.sample.startsWith(this.editor.text)) this.setState('explored');
    });
  };

  onPlay = () => {
    if (this.state === 'typing') this.pause();
    else this.play();
  };

  cancel() {
    this.run += 1;
    window.clearTimeout(this.readyTimer);
    this.editor?.stop();
  }

  pause() {
    this.cancel();
    this.setState('paused');
  }

  async play() {
    if (!this.editor || this.reduced) return;
    if (this.state === 'done' || this.state === 'explored') this.editor.setDoc('');
    const current = this.editor.text;
    if (!this.sample.startsWith(current)) {
      this.setState('explored');
      return;
    }
    const remainder = this.sample.slice(this.editor.text.length);
    const run = ++this.run;
    this.setState('typing');
    this.editor.focus();
    const completed = await this.editor.type(remainder, { seed: 19, median: 42 });
    if (run === this.run && completed) this.setState('done');
  }

  setState(state) {
    this.state = state;
    const labels = {
      ready: this.copy.ready, typing: this.copy.typing, paused: this.copy.paused,
      done: this.copy.done, explored: this.copy.explored, reduced: this.copy.reduced, error: this.copy.error,
    };
    if (this.status) this.status.textContent = labels[state] || this.copy.ready;
    if (this.playButton) {
      this.playButton.textContent = state === 'typing' ? this.copy.pause : state === 'done' || state === 'explored' ? this.copy.replay : this.copy.play;
      this.playButton.disabled = this.reduced || state === 'error';
    }
  }
}

customElements.define('moss-ui-demo', MossUiDemo);
