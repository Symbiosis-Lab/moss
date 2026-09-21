// The marker: an inline Play/Stop button and load-failure line, sitting mid-sentence in the prose
// (stage/README.md, "Authoring"). Toggles its scene on the stage and reflects the stage's own
// report of that scene's state back onto the button, via events bubbled to `document` (see
// moss-stage.js). Knows nothing about how a scene runs.
import { string } from './strings.js';

/**
 * The id of the heading markdown rendered this marker under. Walks up from `el`, scanning each
 * level's earlier siblings for the nearest h1–h6 — a plain sibling search is not enough because
 * moss's Markdown renderer wraps a bare custom element in a lone <p> (see stage/README.md).
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

class MossScene extends HTMLElement {
  connectedCallback() {
    const name = this.getAttribute('name');
    if (!name) {
      console.error('moss-scene: missing a name attribute');
      return;
    }
    this.sceneName = name;
    this.locale = (document.documentElement.lang || 'en').toLowerCase();
    this.playing = false;
    this.failed = false;

    const id = `moss-scene-${++markerSeq}`;

    // Appended, not innerHTML — authored markup may already carry a <noscript> fallback image
    // for readers without JS, and overwriting it here would silently delete their only fallback.
    this.button = document.createElement('button');
    this.button.type = 'button';
    this.button.id = id;
    this.button.className = 'moss-scene__button moss-scene__button--inline';

    // Named by reference to the heading it sits under, not by copying its text (stage/README.md,
    // "Authoring") — no heading, no invented fallback: the button keeps its own visible text as
    // its accessible name and the gap is reported rather than papered over.
    const headingId = findHeadingId(this);
    if (headingId) this.button.setAttribute('aria-labelledby', `${id} ${headingId}`);
    else console.error(`moss-scene: no heading with an id above "${name}" — its button has no heading in its accessible name`);

    // A marker sits mid-sentence, where several may share one heading and so read the same
    // verb+heading accessible name (stage/README.md, "Authoring") — aria-describedby its own
    // paragraph is what keeps assistive tech telling them apart. Several markers in the same
    // paragraph share that paragraph's id rather than each minting their own.
    const paragraph = this.parentElement;
    if (!paragraph.id) paragraph.id = `moss-scene-p-${++markerSeq}`;
    this.button.setAttribute('aria-describedby', paragraph.id);

    this.status = document.createElement('p');
    this.status.className = 'moss-scene__status';
    this.status.setAttribute('role', 'status');

    this.appendChild(this.button);
    this.appendChild(this.status);

    this.hostController = new AbortController();
    const { signal } = this.hostController;
    this.button.addEventListener('click', this.onClick, { signal });
    document.addEventListener('moss-stage-state', this.onStageState, { signal });

    this.render();
  }

  disconnectedCallback() {
    this.hostController?.abort();
  }

  onClick = () => {
    this.dispatchEvent(new CustomEvent(this.playing ? 'moss-scene-stop' : 'moss-scene-play', {
      bubbles: true,
      detail: { name: this.sceneName },
    }));
  };

  onStageState = (event) => {
    if (event.detail.name !== this.sceneName) return;
    this.playing = event.detail.phase === 'playing';
    this.failed = event.detail.phase === 'error';
    this.render();
  };

  render() {
    const verb = string(this.locale, this.playing ? 'stop' : 'play');
    // A filled round ▶/■ plus the word (stage/README.md, "Authoring") — moss-stage.css visually
    // hides the word and shows only the glyph. The glyph is always decorative (aria-hidden); the
    // accessible name always comes from the button's own aria-labelledby self-reference, so the
    // verb text is what that reference reads, hidden or not.
    const glyph = document.createElement('span');
    glyph.className = 'moss-scene__glyph';
    glyph.setAttribute('aria-hidden', 'true');
    glyph.textContent = this.playing ? '■' : '▶';
    // check-docs-stage.mjs's markerState() reads this class to get the bare "Play"/"Stop" text.
    const verbText = document.createElement('span');
    verbText.className = 'moss-scene__verb-text';
    verbText.textContent = verb;
    this.button.replaceChildren(glyph, verbText);
    this.status.textContent = this.failed ? string(this.locale, 'error') : '';
  }
}

customElements.define('moss-scene', MossScene);
