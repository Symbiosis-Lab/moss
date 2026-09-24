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
| code | `--moss-code-accent-primary` | `#2d5a2d` | Code accent primary — moss green, systems/primary languages. Dark: color-mix lightened (fallback #6a9a5a). |
| code | `--moss-code-accent-quaternary` | `#6b5b4f` | Code accent quaternary — deeper brown, config/data |
| code | `--moss-code-accent-secondary` | `#5d5853` | Code accent secondary — warm gray, web languages |
| code | `--moss-code-accent-tertiary` | `#8a8580` | Code accent tertiary — muted brown, scripting |
| code | `--moss-code-background` | `#f8f6f3` | Code block background |
| code | `--moss-code-border` | `#e0ddd6` | Code block border |
| color | `--moss-border-light` | `#e6e2db` | Light border color |
| color | `--moss-border-medium` | `#d1cdc4` | Medium border color |
| color | `--moss-color-accent` | `#2d5a2d` | Links, highlights, accent elements |
| color | `--moss-color-accent-hover` | `color-mix(in oklch, var(--moss-color-accent) 85%, black)` | Accent color on hover — auto-derived from --moss-color-accent. Light: darkens accent by mixing 85% accent + 15% black (hover = press-down cue). Dark: lightens accent by mixing 80% accent + 20% white (hover = surface lift). Override only if the mix produces a wrong result for a custom palette. |
| color | `--moss-color-accent-quiet` | `rgba(45, 90, 45, 0.28)` | Quiet accent for ambient indicators (hairlines, tree-row left-borders). ~28% opacity of moss-color-accent. Consumed by the breadcrumb and preview hairlines. |
| color | `--moss-color-bg` | `#faf8f5` | Page background |
| color | `--moss-color-muted` | `#8a8580` | Secondary/muted text |
| color | `--moss-color-surface` | `#f4f1ec` | Card and surface background |
| color | `--moss-color-text` | `#2c2825` | Primary text |
| color | `--moss-color-text-secondary` | `#5d5853` | Secondary text color — darker than --moss-color-muted, used for readable secondary content (nav links, dates, excerpts). Renamed from --moss-text-secondary in v1.3. |
| color | `--moss-color-ui-accent` | `var(--moss-color-accent)` | Accent for navigation, buttons, and site controls (chrome). Defaults to the content accent; set to a neutral (e.g. var(--moss-color-text)) for quiet chrome. |
| layout | `--moss-container-padding` | `clamp(1rem, 5vw, 2rem)` | Container side padding |
| layout | `--moss-content-width` | `calc(42 * var(--moss-reading-size))` | Maximum content width. Font-stable: a multiple of --moss-reading-size (a length), NOT a ch value, so text, inline images, and figures resolve to the same column at any element font-size (a ch measure drifts per-element and desyncs text from figures when a theme changes the prose font). 42 ≈ the prior 67ch for the default font; tracks the scale-* presets via --moss-reading-size. |
| layout | `--moss-content-width-sidebar` | `calc(39 * var(--moss-reading-size))` | Content width when sidebar is active. Font-stable multiple of --moss-reading-size (≈ prior 62ch); see moss-content-width. |
| layout | `--moss-sidebar-width` | `280px` | Sidebar width |
| layout | `--moss-site-max-width` | `1200px` | Maximum overall site width |
| spacing | `--moss-space-2xl` | `4rem` | Double extra large (64px) |
| spacing | `--moss-space-lg` | `2rem` | Large (32px) |
| spacing | `--moss-space-md` | `1.5rem` | Medium (24px) |
| spacing | `--moss-space-sm` | `1rem` | Small (16px) |
| spacing | `--moss-space-xl` | `3rem` | Extra large (48px) |
| spacing | `--moss-space-xs` | `0.5rem` | Extra small (8px) |
| syntax | `--moss-hl-addition-bg` | `rgba(74, 106, 42, 0.1)` | Diff addition background. Note: dark value only appears in data-theme=dark block in site.css; @media prefers-color-scheme:dark block omits it (pre-existing gap, not introduced here). |
| syntax | `--moss-hl-attr` | `#5a6a4a` | Syntax: attribute |
| syntax | `--moss-hl-builtin` | `#2d5a2d` | Syntax: builtin |
| syntax | `--moss-hl-comment` | `#9a9080` | Syntax: comment |
| syntax | `--moss-hl-deletion` | `#8a4a3a` | Syntax: deletion |
| syntax | `--moss-hl-deletion-bg` | `rgba(138, 74, 58, 0.1)` | Diff deletion background. Note: dark value only appears in data-theme=dark block in site.css; @media prefers-color-scheme:dark block omits it (pre-existing gap, not introduced here). |
| syntax | `--moss-hl-function` | `#3a5a6a` | Syntax: function |
| syntax | `--moss-hl-keyword` | `#7a5834` | Syntax: keyword (also used for tags — intentionally shared) |
| syntax | `--moss-hl-meta` | `#8a7a6a` | Syntax: meta |
| syntax | `--moss-hl-number` | `#6b5b8a` | Syntax: number |
| syntax | `--moss-hl-operator` | `#5d5853` | Syntax: operator |
| syntax | `--moss-hl-string` | `#4a6a2a` | Syntax: string |
| syntax | `--moss-hl-tag` | `#7a5834` | Syntax: tag (same value as moss-hl-keyword — intentionally shared) |
| syntax | `--moss-hl-type` | `#8a5a3a` | Syntax: type |
| typography | `--moss-font-body` | `-apple-system, BlinkMacSystemFont, 'Helvetica Neue', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Noto Sans SC', 'WenQuanYi Micro Hei', sans-serif` | Body text font stack |
| typography | `--moss-font-heading` | `'Iowan Old Style', Athelas, 'Palatino Linotype', Palatino, 'Book Antiqua', 'Songti SC', 'Songti TC', 'STSongti', 'Noto Serif CJK SC', 'Source Han Serif SC', 'NSimSun', 'SimSun', serif` | Heading font stack: serif by default for editorial weight |
| typography | `--moss-font-heading-weight` | `480` | Heading font weight. Source: h1-h6 rule in site.css (font-weight: 480). |
| typography | `--moss-font-mono` | `ui-monospace, 'Cascadia Code', 'SF Mono', SFMono-Regular, 'JetBrains Mono', Menlo, Consolas, 'Liberation Mono', 'Courier New', monospace` | Monospace stack for code |
| typography | `--moss-font-weight-body` | `320` | Default body font weight (renamed from --moss-font-weight in v1.3) |
| typography | `--moss-reading-size` | `var(--moss-reading-size-base)` | Effective reading text size; defaults to --moss-reading-size-base and is adjusted by the reader font-scale control. |
| typography | `--moss-reading-size-base` | `1.125rem` | Body / reading text size. Headings use the --moss-size-lg/-xl/-2xl scale and are unaffected by this token. |
| typography | `--moss-size-2xl` | `1.625rem` | 26px |
| typography | `--moss-size-2xs` | `0.75rem` | 12px — metadata, captions, chip labels |
| typography | `--moss-size-3xl` | `2rem` | 32px |
| typography | `--moss-size-lg` | `1.25rem` | 20px |
| typography | `--moss-size-md` | `var(--moss-reading-size-base)` | 18px reading size — matches --moss-reading-size-base, the body/reading text size |
| typography | `--moss-size-sm` | `1rem` | 16px |
| typography | `--moss-size-xl` | `1.375rem` | 22px |
| typography | `--moss-size-xs` | `0.875rem` | 14px |
<!-- auto:end:css-tokens -->

Run `moss describe --json` for the live token list of your installed moss.
