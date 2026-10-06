---
title: CSS tokens
uid: 5a6a0afc
weight: 41
translationKey: docs-reference-css-tokens
description: Every --moss-* CSS custom property, grouped by category. Run moss describe --json for the live values of your installed moss.
---

These are the CSS custom properties moss defines. Override any of them in `.moss/theme/style.css` — no `!important` needed.

The full size scale runs `--moss-size-{2xs,xs,sm,md,lg,xl,2xl,3xl}`.

<!-- auto:start:css-tokens -->
| Category | Variable | Default | Description |
|---|---|---|---|
| code | `--moss-code-accent-primary` | `#2d5a2d` | Code accent primary — moss green, systems/primary languages. Dark: pinned to #6a9a5a (was color-mix(accent 80%, white)) — since --moss-color-accent now carries its own lightened dark value, re-deriving here would double-lighten the syntax green; pinning keeps code colors stable. |
| code | `--moss-code-accent-quaternary` | `#6b5b4f` | Code accent quaternary — deeper brown, config/data |
| code | `--moss-code-accent-secondary` | `#5d5853` | Code accent secondary — warm gray, web languages |
| code | `--moss-code-accent-tertiary` | `#74706c` | Code accent tertiary — muted brown, scripting |
| code | `--moss-code-background` | `#f8f6f3` | Code block background |
| code | `--moss-code-border` | `#e0ddd6` | Code block border |
| color | `--moss-border-light` | `#e6e2db` | Light border color |
| color | `--moss-border-medium` | `#d1cdc4` | Medium border color |
| color | `--moss-border-strong` | `#8e8b85` | Border for elements whose boundary IS the control — the .moss-input underline. Meets WCAG 1.4.11 non-text contrast (3:1) against the page. Decorative hairlines stay on border-light/medium, which are exempt. |
| color | `--moss-color-accent` | `#2d5a2d` | Links, highlights, accent elements. Dark: lightened forest green (#6a9a5a ≈ 5.3:1 on --moss-color-bg #1c1914) — the raw #2d5a2d is only ~2.2:1 in dark, failing WCAG AA as text/border/icon. #6a9a5a matches the green the codebase already derives for accent-hover/code in dark. |
| color | `--moss-color-accent-hover` | `color-mix(in oklch, var(--moss-color-accent) 85%, black)` | Accent color on hover — auto-derived from --moss-color-accent. Light: darkens accent by mixing 85% accent + 15% black (hover = press-down cue). Dark: lightens accent by mixing 80% accent + 20% white (hover = surface lift). Override only if the mix produces a wrong result for a custom palette. |
| color | `--moss-color-accent-quiet` | `rgba(45, 90, 45, 0.28)` | Quiet accent for ambient indicators (hairlines, tree-row left-borders). ~28% opacity of moss-color-accent. Consumed by the breadcrumb and preview hairlines (the ambient renderer). Ambient only — never a focus indicator: it composites too close to the page to satisfy 1.4.11. Use moss-color-ui-accent for focus. |
| color | `--moss-color-bg` | `#faf8f5` | Page background |
| color | `--moss-color-muted` | `#716d69` | Secondary/muted text. Light value meets 4.5:1 on bg, surface and code backgrounds (WCAG 1.4.3) — it is body text at 14px in ~32 places, not decoration. |
| color | `--moss-color-surface` | `#f4f1ec` | Card and surface background |
| color | `--moss-color-text` | `#2c2825` | Primary text |
| color | `--moss-color-text-secondary` | `#5d5853` | Secondary text color — darker than --moss-color-muted, used for readable secondary content (nav links, dates, excerpts). Renamed from --moss-text-secondary in v1.3. |
| color | `--moss-color-ui-accent` | `var(--moss-color-accent)` | Accent for navigation, buttons, and site controls (chrome). Defaults to the content accent; set to a neutral (e.g. var(--moss-color-text)) for quiet chrome. |
| elevation | `--moss-edge-light` | `rgba(255, 255, 255, 0.45)` | The hairline highlight on the edge that faces the light. Nearly invisible on paper; on the dark ground it is what lifts a surface, since the shadow alone cannot. |
| elevation | `--moss-elevation-1` | `inset calc(var(--moss-light-x) * -1px) calc(var(--moss-light-y) * -1px) 0 var(--moss-edge-light), calc(var(--moss-light-x) * -6.8px) calc(var(--moss-light-y) * -6.8px) 4.8px 0.8px color-mix(in srgb, var(--moss-shadow-color) calc(32% * var(--moss-shadow-strength)), transparent)` | A resting control on its parent: pills, toggle thumbs, close buttons, a hovered card. The lit edge plus Ambient's sheet cast at elevation 1: 6.8px along the light, 4.8px blur, 32% × strength. |
| elevation | `--moss-elevation-2` | `inset calc(var(--moss-light-x) * -1px) calc(var(--moss-light-y) * -1px) 0 var(--moss-edge-light), calc(var(--moss-light-x) * -13.6px) calc(var(--moss-light-y) * -13.6px) 9.6px 0.8px color-mix(in srgb, var(--moss-shadow-color) calc(24% * var(--moss-shadow-strength)), transparent)` | Floating chrome over the shell or page: tooltips, menus, popovers, toasts, the nav island. Elevation 2: 13.6px along the light, 9.6px blur, 24% × strength. |
| elevation | `--moss-elevation-3` | `inset calc(var(--moss-light-x) * -1px) calc(var(--moss-light-y) * -1px) 0 var(--moss-edge-light), calc(var(--moss-light-x) * -20.4px) calc(var(--moss-light-y) * -20.4px) 14.4px 0.8px color-mix(in srgb, var(--moss-shadow-color) calc(16% * var(--moss-shadow-strength)), transparent)` | A layer over everything, usually with a backdrop: modal, command palette. Elevation 3: 20.4px along the light, 14.4px blur, 16% × strength. |
| elevation | `--moss-light-x` | `0` | Where the light comes from, horizontally: -1 is the left edge, 1 the right, 0 straight above. Shadows fall the other way. |
| elevation | `--moss-light-y` | `-0.5` | Where the light comes from, vertically: -1 is above, 1 below. moss is lit from directly above, like paper under a lamp. |
| elevation | `--moss-shadow-color` | `#2b2117` | The colour of every shadow before opacity. Warm near-black on paper, because a black shadow on a warm ground is the wrong colour; black on the dark ground. |
| elevation | `--moss-shadow-strength` | `0.5` | Multiplier on every shadow opacity. A dark ground needs a much stronger shadow to read at all. |
| layout | `--moss-container-padding` | `clamp(1rem, 5vw, 2rem)` | Container side padding |
| layout | `--moss-content-width` | `calc(42 * var(--moss-reading-size))` | Maximum content width. Font-stable: a multiple of --moss-reading-size (a length), NOT a ch value, so text, inline images, and figures resolve to the same column at any element font-size (a ch measure drifts per-element and desyncs text from figures when a theme changes the prose font). 42 ≈ the prior 67ch for the default font; tracks the scale-* presets via --moss-reading-size. |
| layout | `--moss-content-width-sidebar` | `calc(39 * var(--moss-reading-size))` | Content width when sidebar is active. Font-stable multiple of --moss-reading-size (≈ prior 62ch); see moss-content-width. |
| layout | `--moss-sidebar-width` | `280px` | Sidebar width |
| layout | `--moss-site-max-width` | `1200px` | Maximum overall site width |
| layout | `--moss-width-wide` | `min(calc(56 * var(--moss-reading-size)), var(--moss-site-max-width))` | Width of a data-width="wide" content band. Font-stable multiple of --moss-reading-size like moss-content-width; the min() clamp is load-bearing — the reader font-scale control scales --moss-reading-size (scale-xlarge = 1.25x), and without the clamp a wide band at xlarge overruns --moss-site-max-width and stops aligning with the nav. data-width="page" reuses --moss-site-max-width directly (it must stay identical to the .container width or the band stops aligning with nav/footer); there is deliberately no second token for it. |
| spacing | `--moss-space-2xl` | `4rem` | Double extra large (64px) |
| spacing | `--moss-space-lg` | `2rem` | Large (32px) |
| spacing | `--moss-space-md` | `1.5rem` | Medium (24px) |
| spacing | `--moss-space-sm` | `1rem` | Small (16px) |
| spacing | `--moss-space-xl` | `3rem` | Extra large (48px) |
| spacing | `--moss-space-xs` | `0.5rem` | Extra small (8px) |
| syntax | `--moss-hl-addition-bg` | `rgba(74, 106, 42, 0.1)` | Diff addition background. |
| syntax | `--moss-hl-attr` | `#5a6a4a` | Syntax: attribute |
| syntax | `--moss-hl-builtin` | `#2d5a2d` | Syntax: builtin |
| syntax | `--moss-hl-comment` | `#787064` | Syntax: comment. Both values meet 4.5:1 on the code background — a comment is text a reader is expected to read, so it is not exempt as decoration. |
| syntax | `--moss-hl-deletion` | `#8a4a3a` | Syntax: deletion |
| syntax | `--moss-hl-deletion-bg` | `rgba(138, 74, 58, 0.1)` | Diff deletion background. |
| syntax | `--moss-hl-function` | `#3a5a6a` | Syntax: function |
| syntax | `--moss-hl-keyword` | `#7a5834` | Syntax: keyword (also used for tags — intentionally shared) |
| syntax | `--moss-hl-meta` | `#7c6e5f` | Syntax: meta |
| syntax | `--moss-hl-number` | `#6b5b8a` | Syntax: number |
| syntax | `--moss-hl-operator` | `#5d5853` | Syntax: operator |
| syntax | `--moss-hl-string` | `#4a6a2a` | Syntax: string |
| syntax | `--moss-hl-tag` | `#7a5834` | Syntax: tag (same value as moss-hl-keyword — intentionally shared) |
| syntax | `--moss-hl-type` | `#8a5a3a` | Syntax: type |
| typography | `--moss-font-body` | `-apple-system, BlinkMacSystemFont, 'Helvetica Neue', 'Moss PingFang SC', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Noto Sans SC', 'WenQuanYi Micro Hei', sans-serif` | Body text font stack. 'Moss PingFang SC' is the weight-pinned PingFang @font-face defined in site.css (see the PingFang weight pin comment there). |
| typography | `--moss-font-heading` | `'Source Han Serif SC', 'Noto Serif CJK SC', 'Songti SC', 'STSongti', 'NSimSun', 'SimSun', 'Source Serif 4', 'Source Serif Pro', 'Times New Roman', Times, serif` | Heading font stack: serif by default for editorial weight. Han faces lead, because CSS fallback is per-glyph: with Latin serifs in front, every mixed heading drew its Latin from one file and its Han from another. The Latin fallbacks behind them are high-contrast transitionals (Source Serif, Times) — the closest Latin match for a Ming's hairline horizontals, where Iowan/Palatino are as unlike 宋體 as a serif gets. This mitigates the split; it does not remove it (a heading set in a Han face still borrows that face's own Latin). Only a webfont covering both scripts does that. |
| typography | `--moss-font-heading-weight` | `480` | Weight for EVERY heading — the h1-h6 rule in site.css and article section headings alike. Article subheads used to pin 500 of their own, which made this token an authority over some headings and not others; on the CJK sans the two values are one face anyway (the PingFang pin resolves 400-449 to Regular, so 480 and 500 both land on Medium). |
| typography | `--moss-font-mono` | `ui-monospace, 'Cascadia Code', 'SF Mono', SFMono-Regular, 'JetBrains Mono', Menlo, Consolas, 'Liberation Mono', 'Courier New', monospace` | Monospace stack for code |
| typography | `--moss-font-weight-body` | `320` | Default body font weight (renamed from --moss-font-weight in v1.3) |
| typography | `--moss-read-caption` | `calc(var(--moss-reading-size) * 0.722)` | Figure caption — 13px at the default reading size. See --moss-read-title for what the ratio is measured against. |
| typography | `--moss-read-h1` | `calc(var(--moss-reading-size) * 1.667)` | Article `#` section heading — 30px at the default reading size. |
| typography | `--moss-read-h2` | `calc(var(--moss-reading-size) * 1.333)` | Article `##` section heading — 24px at the default reading size. |
| typography | `--moss-read-h3` | `calc(var(--moss-reading-size) * 1.125)` | Article `###` section heading — 19px at the default reading size. Deliberately close to body: at this depth the level is carried by weight and the space above it, not by competing on size. |
| typography | `--moss-read-h4` | `calc(var(--moss-reading-size) * 1.028)` | Article `####` section heading — 18.5px at the default reading size. |
| typography | `--moss-read-h5` | `calc(var(--moss-reading-size) * 0.889)` | Article `#####` section heading — 16px at the default reading size. |
| typography | `--moss-read-h6` | `calc(var(--moss-reading-size) * 0.778)` | Article `######` section heading — 14px at the default reading size. |
| typography | `--moss-read-leading` | `1.75` | Body line-height, unitless. The Latin default; html[lang^="zh"] overrides it to 1.8 (CJK needs looser leading — see that rule in site.css) rather than setting line-height on body directly, so --moss-read-line (below) picks up the bump for both scripts from one place. |
| typography | `--moss-read-line` | `calc(var(--moss-reading-size) * var(--moss-read-leading))` | One line of body text — the unit of prose vertical spacing (article paragraph/heading/figure margins), the way --moss-reading-size is the unit of prose type size. Before this token, --moss-space-* rem chrome geometry did that job: fixed while body text ran 16px-24px across the four reader font-scale steps, so the paragraph gap shrank from 0.78 line to 0.56 line as the reader enlarged text, and the gap above a `##` tied the figure gap instead of standing apart from it. |
| typography | `--moss-read-title` | `calc(var(--moss-reading-size) * 2.222)` | Article / folder masthead — 40px at the default reading size. Every prose size is a ratio of --moss-reading-size, so the reader's font-scale control and the CJK size bump move the whole ladder together instead of sliding body copy past fixed headings. A theme retunes prose by overriding these ratios; --moss-size-* stays fixed because it is chrome geometry (the nav island reserves against --moss-size-lg). |
| typography | `--moss-reading-script-scale` | `1` | Per-script multiplier folded into --moss-reading-size. site.css sets 1.06 for html[lang^="zh"]: CJK ideographs fill the em square, so they need ~+1px over Latin at the same nominal size. It lives on the reading size rather than on `body { font-size }` so headings and captions get the same bump. |
| typography | `--moss-reading-size` | `calc(var(--moss-reading-size-base) * var(--moss-reading-script-scale) * var(--moss-reading-step))` | Effective reading text size, and the base of the whole prose scale (--moss-read-*): --moss-reading-size-base times the per-script multiplier times the reader's font-scale step (--moss-reading-step). |
| typography | `--moss-reading-size-base` | `1.125rem` | Body / reading text size before the per-script multiplier and the reader font-scale step. Prose headings and captions are ratios of the result (--moss-read-*), so they follow this token; --moss-size-* is chrome and does not. |
| typography | `--moss-reading-step` | `1` | The reader's font-scale control, as a bare multiplier — the one variable site.css's `html.scale-*` classes write (0.89/1.12/1.25). Folded into --moss-reading-size below rather than left for each `html.scale-*` rule to re-derive the whole formula, so a second base (vertical typesetting's own body text, sized from --moss-vertical-font-size instead of --moss-reading-size-base) can multiply by this same token and pick up the control too, instead of the control silently doing nothing there. |
| typography | `--moss-size-2xl` | `1.625rem` | 26px |
| typography | `--moss-size-2xs` | `0.75rem` | 12px — metadata, captions, chip labels |
| typography | `--moss-size-lg` | `1.25rem` | 20px |
| typography | `--moss-size-md` | `1.125rem` | 18px. Chrome geometry, deliberately a fixed rem: it used to alias --moss-reading-size-base, which made it the one --moss-size-* that moved with the reading scale, so a non-prose h4 shrank on mobile while every sibling token held. Prose reads --moss-read-* instead. |
| typography | `--moss-size-sm` | `1rem` | 16px |
| typography | `--moss-size-xl` | `1.375rem` | 22px |
| typography | `--moss-size-xs` | `0.875rem` | 14px |
| typography | `--moss-vertical-font-size` | `clamp(1rem, 1.9svh, 1.4rem)` | Body font size under vertical typesetting (data-typesetting="vertical"), read by crates/moss-build/src/assets/css/site/vertical.css. Scales with viewport height rather than width so the 38em column measure keeps the same share of the page on a short and a tall screen. The site-tunable knob for vertical type size; column placement (golden-section head margin) is a separate, non-tunable moss default. |
<!-- auto:end:css-tokens -->

Run `moss describe --json` for the live token list of your installed moss.
