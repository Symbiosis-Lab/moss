/**
 * Typed event contract between the selection tooltip (dispatcher in
 * `selection-actions.ts`) and the comment float-shell (listener in
 * `quote-float.ts`). Both sides bind via these helpers so TypeScript
 * catches breakage when either end renames the event or changes its shape.
 *
 * Lives in `comments/` because the event is owned by the comment surface,
 * but is intentionally importable from the parent `site/` directory.
 */

export const QUOTE_COMMENT_EVENT = "moss:quote-comment" as const;

export interface QuoteCommentDetail {
  text: string;
}

export function dispatchQuoteComment(text: string): void {
  document.dispatchEvent(
    new CustomEvent<QuoteCommentDetail>(QUOTE_COMMENT_EVENT, {
      detail: { text },
    })
  );
}

export type QuoteCommentHandler = (detail: QuoteCommentDetail) => void;

export function onQuoteComment(handler: QuoteCommentHandler): () => void {
  const listener = (e: Event): void => {
    const detail = (e as CustomEvent<QuoteCommentDetail>).detail;
    if (detail) handler(detail);
  };
  document.addEventListener(QUOTE_COMMENT_EVENT, listener);
  return () => document.removeEventListener(QUOTE_COMMENT_EVENT, listener);
}
