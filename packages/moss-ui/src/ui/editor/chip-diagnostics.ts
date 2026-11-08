// chip-diagnostics.ts — frontmatter diagnostics, addressed to the chip that owns them.
//
// A PULL adapter over `validateContent`: no
// state of its own beyond the last answer, one RPC per file load, and the
// message text comes verbatim from Rust (`EditorDiagnostic.message`), which is
// where the locale table lives for backend-composed strings.
//
// It exists because `validateContent` had no production consumer. The command,
// `EditorDiagnostic`, and `moss_core::validation` were all written and gated
// and rendered nowhere, so everything they already detect — a wrong field
// type, a missing required field, and the FOREIGN_FIELD_HINTS table that
// answers `slug:` with "did you mean `url:`?" — was invisible to authors. The
// `url:` collision that sent a real site's pages to /awards/comics-2/ just
// made the gap concrete enough to close.

// The diagnostic shape this module reads. moss's generated `EditorDiagnostic`
// satisfies it structurally, the same arrangement cm-problems uses for the
// reference envelope. The RPC itself is injected, so this module calls no
// command of its own.
export interface FieldDiagnostic {
  severity: number;
  message: string;
  path: string | null;
}

/** How the caller reaches `validate_content`. */
export type ValidateFn = (filePath: string) => Promise<
  { status: 'ok'; data: FieldDiagnostic[] } | { status: 'error'; error: unknown }
>;

/** LSP severities, as `FieldDiagnostic.severity` carries them. */
export const SEVERITY_ERROR = 1;
export const SEVERITY_WARNING = 2;

export class ChipDiagnostics {
  private byField = new Map<string, FieldDiagnostic>();

  constructor(private validate: ValidateFn) {}

  /**
   * Re-read diagnostics for `filePath`. Clears first, so a file with none —
   * or a file that could not be validated at all — never shows the previous
   * file's complaint against its own fields.
   *
   * Never throws: a diagnostic surface that can break the editor around it is
   * worse than one that is occasionally absent.
   */
  async load(filePath: string | null): Promise<void> {
    this.byField.clear();
    if (!filePath) return;
    try {
      const result = await this.validate(filePath);
      if (result.status !== 'ok') return;
      for (const d of result.data) {
        // Keep the FIRST per field: severities arrive worst-first from Rust
        // (errors before hints), and a chip has room for one line.
        if (d.path && !this.byField.has(d.path)) this.byField.set(d.path, d);
      }
    } catch (e) {
      console.warn('[chip-diagnostics] validation unavailable', e);
    }
  }

  /**
   * Load `filePath`'s diagnostics and mark the chips in `containerId`.
   *
   * Runs after chip-bar's render rather than inside it: the mark is a DOM
   * pass, so chip-bar.ts — the editor's largest file — needs no branch of its
   * own. A later chip edit re-renders and drops the mark, which is the
   * behaviour to want: the author is fixing it, and the next build says
   * whether she did.
   */
  async refresh(filePath: string, containerId: string): Promise<void> {
    await this.load(filePath);
    this.decorate(document.getElementById(containerId));
  }

  /** The diagnostic to render on `key`'s chip, if any. */
  for(key: string): FieldDiagnostic | null {
    return this.byField.get(key) ?? null;
  }

  /**
   * Mark the chips in `container` that carry a diagnostic.
   *
   * A DOM pass over `[data-key]` rather than a branch inside `mkChip`: chips
   * already publish their field key, `mkChip` has six return points, and
   * chip-bar.ts is the editor's largest file — a decoration that can be
   * applied from outside should be.
   *
   * `data-tooltip`, never `title` (moss UI convention). Idempotent: clears
   * every mark first, so a re-render or a fixed field leaves nothing behind.
   */
  decorate(container: HTMLElement | null): void {
    if (!container) return;
    for (const el of container.querySelectorAll<HTMLElement>('.chip[data-key]')) {
      const d = el.dataset.key ? this.for(el.dataset.key) : null;
      el.classList.toggle('chip--diagnostic', !!d);
      el.classList.toggle('chip--error', d?.severity === SEVERITY_ERROR);
      if (d) {
        el.dataset.tooltip = d.message;
        // A chip is a click target, not a heading, so the message has to reach
        // a screen reader some other way than by being read as content.
        el.setAttribute('aria-description', d.message);
      } else {
        delete el.dataset.tooltip;
        el.removeAttribute('aria-description');
      }
    }
  }
}
