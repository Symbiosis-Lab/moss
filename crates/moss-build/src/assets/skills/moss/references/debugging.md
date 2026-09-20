# Debugging a moss site

## Logs

| Platform | Path |
|----------|------|
| macOS    | `~/Library/Logs/host.moss.publisher/` |
| Windows  | `%LOCALAPPDATA%\host.moss.publisher\logs\` |
| Linux    | `~/.local/share/host.moss.publisher/logs/` |

The table is where the desktop app keeps its logs; `moss build` logs to stderr instead, and only problems unless `MOSS_LOG_LEVEL` asks for more: `warn` is the default, so a build that prints nothing had nothing to report, not timings it kept back. `error` prints less; `info` adds the `build.summary` line (per-phase milliseconds) and a few summary lines (`[slots]`, `[search]`, `[cache]`, two `[render]`); `debug` adds the per-step `[render]`, `[reduce]`, `[scan]` and `[build]` timings. Chasing a slow build: `MOSS_LOG_LEVEL=debug moss build <folder>`. `RUST_LOG` is not read.

## Isolating plugin vs. core failures

```
moss build <folder> --no-plugins
```

If the problem disappears, a plugin is the cause. There is no CLI switch for
one plugin at a time — `--no-plugins` is all-or-nothing — so narrow it from the
build output, which names each plugin as it runs, or turn individual plugins
off in Settings.

## Viewing the built output

```
moss build <folder> --serve
```

Builds the site and serves it on port 8080, or the next free port if 8080 is
taken — read the port from the build output rather than assuming it, or you
may be inspecting a different moss instance's site. Inspect the built HTML under
`.moss/build/current/` (the active frozen generation). Useful for headless or automated checks without
opening the GUI preview. The `.css` files under its `_moss/` are minified build
output — don't read or edit them; use `moss describe --css <selector>` instead.

## Shortcode appears as literal text

Usually this means **no matching closing fence**. The closer carries the same
colon count as the opener (`::::grid` closes with `::::`), and nothing but
whitespace may follow it — `::: extra` is body text, not a closer.

**Nested fences must not share a colon count.** A closer matches its opener by
arity, so `::::grid` wrapping `:::gallery` and `:::grid` wrapping `::::buttons`
both work — the direction does not matter, only that the two differ. Give both
the same count and the first bare `:::` closes the *outer* fence early, so the
rest of the block spills out. This is the most common way a correct-looking
shortcode produces wrong output.

Two other causes produce the same symptom: the line sits somewhere fences are
inert (inside a code fence, an indented code block, an inline code span, or an
HTML comment), or it is not a valid opener at all — `:::` with no name and no
`{` is just text.

A **misspelled name does not render literally.** `:::gallry` still parses; it
renders as `<div class="moss-unknown-shortcode" data-name="gallry">` wrapping
your body, and the build prints:

```
[essays/piece.md] unknown shortcode `:::gallry`
```

So a block that came out as an unstyled div is a name problem, not a fence
problem. If you have lost the build log, grep the output instead:

```
grep -ro 'moss-unknown-shortcode[^>]*' .moss/build/current/
```

`data-name` is the name moss did not recognize. For the valid names, run
`moss describe --json` and take the components with `authorable: true` — the
shortcode name is the `class` minus its `moss-` prefix (`moss-hero` →
`:::hero`), and each entry's `example_markdown` shows the exact syntax.

## Import stopped mid-crawl

`moss import --recursive` caps at 200 pages. If you get a partial result,
check the logs for HTTP errors on individual pages, split the crawl by
path prefix, and re-import the missing sections into the same folder.

## A frontmatter field didn't take effect

A value that cannot satisfy its typed field is dropped **alone** — every other
key survives (ADR-020). The build prints one line per dropped field:

```
[essays/piece.md] frontmatter: weight: invalid type: string "high", expected i32
```

So check the build log for `frontmatter:` lines first. The usual cause is a
field given the wrong type (`weight: high` — `weight` is an integer). Scalars
are coerced wherever it is unambiguous, so a numeric `uid:` or `title:` (e.g.
`uid: 12345`) works unquoted and needs no fix.
