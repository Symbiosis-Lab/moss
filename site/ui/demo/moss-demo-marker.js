// The marker: an inline Play/Stop button and load-failure line, sitting mid-sentence in the prose
// (site/ui/demo/README.md, "Authoring"). Toggles its scene on the nearest demo frame and reflects
// the frame's own report of that scene's state back onto the button, via events bubbled to
// `document` (see demo-frame.js). Knows nothing about how a scene runs.
//
// Authors never write this element: `.moss/theme/script.js` upgrades a plain
// `a[href^="#scene="]` link into one of these (site/ui/demo/README.md, "Markdown markers") — the
// upgrade is what supplies the `name` attribute (the part of the href after `#scene=`, empty for
// "just open the surface").
import { string } from './strings.js';

/**
 * The id of the heading markdown rendered this marker under. Walks up from `el`, scanning each
 * level's earlier siblings for the nearest h1–h6 — a plain sibling search is not enough because
 * moss's Markdown renderer wraps a bare custom element in a lone <p> (see site/ui/demo/README.md).
 */
function findHeadingId(el) {
  let node = el;
  while (node && node.tagName !== 'ARTICLE' && node !== document.body) {
    for (let sib = node.previousElementSibling; sib; sib = sib.previousElementSibling) {
      if (/^H[1-6]$/.test(sib.tagName)) return sib.id || '';
    }
    node = node.parentElement;
  }
  return '';
}

let markerSeq = 0;

class MossDemoMarker extends HTMLElement {
  connectedCallback() {
    // `null` (the attribute is altogether missing) is the authoring bug this guards against — an
    // empty string (`#scene=`, "just open the surface") is a deliberate, valid name.
    const name = this.getAttribute('name');
    if (name === null) {
      console.error('moss-demo-marker: missing a name attribute');
      return;
    }
    this.sceneName = name;
    this.locale = (document.documentElement.lang || 'en').toLowerCase();
    this.playing = false;
    this.failed = false;

    const id = `moss-demo-marker-${++markerSeq}`;

    this.button = document.createElement('button');
    this.button.type = 'button';
    this.button.id = id;
    this.button.className = 'moss-demo-marker__button moss-demo-marker__button--inline';

    // Named by reference to the heading it sits under, not by copying its text (site/ui/demo/
    // README.md, "Authoring") — no heading, no invented fallback: the button keeps its own visible
    // text as its accessible name and the gap is reported rather than papered over.
    const headingId = findHeadingId(this);
    if (headingId) this.button.setAttribute('aria-labelledby', `${id} ${headingId}`);
    else console.error(`moss-demo-marker: no heading with an id above "${name}" — its button has no heading in its accessible name`);

    // A marker sits mid-sentence, where several may share one heading and so read the same
    // verb+heading accessible name (site/ui/demo/README.md, "Authoring") — aria-describedby its
    // own paragraph is what keeps assistive tech telling them apart. Several markers in the same
    // paragraph share that paragraph's id rather than each minting their own.
    const paragraph = this.parentElement;
    if (!paragraph.id) paragraph.id = `moss-demo-marker-p-${++markerSeq}`;
    this.button.setAttribute('aria-describedby', paragraph.id);

    this.status = document.createElement('p');
    this.status.className = 'moss-demo-marker__status';
    this.status.setAttribute('role', 'status');

    this.appendChild(this.button);
    this.appendChild(this.status);

    this.hostController = new AbortController();
    const { signal } = this.hostController;
    this.button.addEventListener('click', this.onClick, { signal });
    document.addEventListener('moss-demo-state', this.onDemoState, { signal });

    this.render();
  }

  disconnectedCallback() {
    this.hostController?.abort();
  }

  onClick = () => {
    this.dispatchEvent(new CustomEvent(this.playing ? 'moss-demo-stop' : 'moss-demo-play', {
      bubbles: true,
      detail: { name: this.sceneName },
    }));
  };

  onDemoState = (event) => {
    if (event.detail.name !== this.sceneName) return;
    this.playing = event.detail.phase === 'playing';
    this.failed = event.detail.phase === 'error';
    this.render();
  };

  render() {
    const verb = string(this.locale, this.playing ? 'stop' : 'play');
    // A filled round ▶/■ plus the word (site/ui/demo/README.md, "Authoring") — demo.css visually
    // hides the word and shows only the glyph. The glyph is always decorative (aria-hidden); the
    // accessible name always comes from the button's own aria-labelledby self-reference, so the
    // verb text is what that reference reads, hidden or not.
    const glyph = document.createElement('span');
    glyph.className = 'moss-demo-marker__glyph';
    glyph.setAttribute('aria-hidden', 'true');
    glyph.textContent = this.playing ? '■' : '▶';
    // check-docs-demo.mjs's markerState() reads this class to get the bare "Play"/"Stop" text.
    const verbText = document.createElement('span');
    verbText.className = 'moss-demo-marker__verb-text';
    verbText.textContent = verb;
    this.button.replaceChildren(glyph, verbText);
    this.status.textContent = this.failed ? string(this.locale, 'error') : '';
  }
}

customElements.define('moss-demo-marker', MossDemoMarker);
