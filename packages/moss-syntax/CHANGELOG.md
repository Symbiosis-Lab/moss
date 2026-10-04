# Changelog

All notable changes to this package are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `parseWikilinkPhase` (`completion-core`) gains a `'width'` `CompletionPhase` variant: a `|` typed inside an OPEN embed target (`![[cover.png|5`) is a width percentage, not free-text alias. A non-embed wikilink's `|alias` is unaffected — aliases stay the `wikilink` phase.

### Removed

- Document-wide "source mode" and the editor-focus reveal-suspension state (`cm6`): `isSourceMode`, `setSourceModeEffect`, `sourceModeField`, `sourceModeChanged`, `revealInputsChanged`, `revealInputsChangedIn`, `editorFocusField`, `setEditorFocusedEffect`, `isRevealSuspended`, `revealSuspensionChanged`, and `cm-active-lines`'s `getActiveLines`/`isNodeActive`/`spanOnActiveLine`. Presentation no longer depends on the caret at all; `nodeTouchesSelection` is the one predicate left, and it now gates only colour-only feedback (unresolved-link dimming, lint), never rendering.
- The shortcode-block resting/active split and its micro-tag rendering (`cm6/cm-shortcode-block`): `ResolveShortcodeAsset`, `ShortcodeAsset`, `classListLabel`, `tagParams`, and the `resolveAsset` option on `shortcodeBlockExtension`. A `:::name {attrs}` fence now always renders as token-highlighted text with its delimiters in the margin, never as a one-line collapsed tag — there is no icon slot left for a resolved thumbnail to fill.
- `cm-footnote`'s `footnoteDecorations` no longer takes an `activeLines` parameter — it never reads the selection.

### Changed

- A footnote reference (`[^1]`) and a shortcode fence's delimiters now render identically regardless of caret position; previously both revealed their raw source characters while the caret was inside them.
- A shortcode fence's `:::` characters and a `+++` cell divider are no longer tagged `cm-hang`/`cm-hung`: they stay in the text column, muted by `cm-sc-delim`. Only a heading's or quote's marker hangs into the margin, and the consumer's CSS no longer needs an override to undo it.

### Fixed

- Two footnote definitions on adjacent lines (`[^1]: a` then `[^b]: b`, no blank line) are two notes, as in the build. The second line was folded into the first as lazy paragraph continuation, so `[^b]` had no definition and its references were left unstyled.
