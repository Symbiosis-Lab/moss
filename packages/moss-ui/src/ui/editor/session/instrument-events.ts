/**
 * instrument-events.ts — what the editor and the shell say to each other while
 * an editing instrument is open on a hero. The editor holds the caret and does
 * every write; the shell can see the preview and answers with what it sees.
 * A hero is named by the file and the line its `:::hero` fence opens on (the
 * number its `data-source-range` starts with); any other picture, by its own
 * line (its `data-source-line`).
 */

/** editor → shell: how big is this picture's frame right now? */
export const HERO_FRAME_REQUEST = 'instrument:hero-frame-request';
/** shell → editor: the answer, in preview CSS px. Not sent when the picture is not on the page. */
export const HERO_FRAME = 'instrument:hero-frame';
/**
 * editor → shell: show the picture cropped to `position` ("X% Y%"), or `null` to put it back.
 * `held` marks the value a release just wrote: the shell keeps it until a rebuilt page shows it.
 */
export const HERO_IMAGE_FOCUS = 'instrument:hero-image-focus';

/** editor → shell: the caret is on this hero's opening line (`null`: it is not, or the editor is gone). */
export const HERO_WORDS = 'instrument:hero-words';
/** shell → editor: the hero's words were dropped at start, centre or end; rewrite its opening line. */
export const HERO_ALIGN = 'instrument:hero-align';

export const HERO_ALIGNS = ['start', 'center', 'end'] as const;
export type HeroAlign = (typeof HERO_ALIGNS)[number];

/** `path` is the editor file's project-relative path: the shell acts only when that file is the page it previews. */
export interface HeroFrameRequest { path: string; line: number }
export interface HeroFrame { line: number; width: number; height: number }
export interface HeroImageFocus { path: string; line: number; position: string | null; held?: boolean }
export interface HeroWordsAt { path: string; line: number | null }
export interface HeroAlignRequest { line: number; align: HeroAlign }

/** A line number that crossed a webview boundary, before it reaches a selector. */
export const isLine = (n: unknown): n is number => Number.isInteger(n) && (n as number) > 0;
