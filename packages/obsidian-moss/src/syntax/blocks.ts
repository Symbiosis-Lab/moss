// The `:::name` blocks a page can hold, and which of them a Live Preview editor may swap for a render.
import { SHORTCODES } from "@symbiosis-lab/moss-syntax";
import { scanShortcodeBlocks, type ShortcodeBlock } from "./shortcode-parser";

/** moss's own blocks an author can write. */
export const BLOCKS = SHORTCODES.filter((block) => block.authorable);

const NAMES: ReadonlySet<string> = new Set(BLOCKS.map((block) => block.name));

/** The closed top-level blocks moss renders, as `[from, to)` offsets of whole fence-to-fence text. */
export function topBlocks(text: string): ShortcodeBlock[] {
  return scanShortcodeBlocks(text).filter((block) => block.closeFrom !== null && NAMES.has(block.name));
}

/** A block shows its source while the selection touches it, and is replaced by its render otherwise. */
export function isEditing(block: Pick<ShortcodeBlock, "from" | "to">, selection: readonly { from: number; to: number }[]): boolean {
  return selection.some((range) => range.from <= block.to && range.to >= block.from);
}
