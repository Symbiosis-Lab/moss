# A site-wide date style

**Status:** Proposed. The setting and the formatting live in this repository; its row in the app's site settings is drawn by the desktop app from the same field table.

## What it is

One setting that chooses how every visible date on a site is written, beside `typesetting` under `[site]`:

```toml
[site]
date_style = "iso"
```

| Value | A full date | A month | A year |
|---|---|---|---|
| `written` (default, today's behaviour) | April 10, 1919 · 10. April 1919 · 1919年4月10日 | April 1919 · 1919年4月 | 1919 |
| `iso` | 1919-04-10 | 1919-04 | 1919 |

`iso` writes a date to the precision the page gave it and never invents a day or a month. It reads the same in English, German, Simplified and Traditional Chinese: digits and hyphens, no month name, no 年月日.

## Why a site setting

A site that wants ISO dates today copies each `<time datetime>` value into its text with a theme script, so the dates change after the page paints and a reader without script sees the other form. A per-listing option would have to be repeated on every embed and would still leave the article's own date line, the series links and the comments in the written form. The choice is a matter of the site's voice, like its typesetting, so it is made once.

## What it changes

The places moss writes a date for a reader go through the formatters in `build/components/date.rs`, which already take the page's language and typesetting. Comments are the exception: they format their dates with a copy of their own, which this change folds into the same formatters.

Two dates are written in the browser: a comment that arrives after the build, and a country map's stamp showing the reader's own day. Both scripts read one attribute, `data-date-style="iso"` on `<html>`, which the page carries only under `iso`, so the rule has one copy in the build and one in script.

| Place | `written` | `iso` |
|---|---|---|
| An article's date line, a field of kind `date` | April 10, 1919 | 1919-04-10 |
| Listing cards and rows | April 1919 | 1919-04-10 |
| A row under a year heading | 04 | 04-10 |
| A stream entry, an event's day and time | April 10, 1919, 19:30 | 1919-04-10 19:30 |
| A comment's date | April 10, 1919 | 1919-04-10 |
| A country map's "As of" stamp, the reader's own date included | April 10, 1919 | 1919-04-10 |

A listing under `iso` shows the day, where the written form drops it: the written month reads as a date, and `1919-04` beside `1919-04-10` in one list reads as missing data.

It never changes a machine-readable date: `datetime` attributes, feeds, the sitemap and structured data keep their own formats.

## Vertical Chinese pages

On a `typesetting = "vertical"` page in Chinese, dates are written in Chinese numerals (一九一九年·四月十日) because Arabic digits lie on their side in a vertical line. That rule wins over `iso`: such a page keeps its numerals, and the build prints one note saying so. A site that mixes horizontal and vertical pages gets ISO dates on the horizontal ones.

## The four languages

The setting's name and values are the same in every language, and its output is too. Only its row in the app's site settings is translated:

| | Label | `written` | `iso` |
|---|---|---|---|
| English | Date style | Written out | ISO (1919-04-10) |
| German | Datumsformat | Ausgeschrieben | ISO (1919-04-10) |
| Simplified Chinese | 日期样式 | 文字 | ISO（1919-04-10） |
| Traditional Chinese | 日期樣式 | 文字 | ISO（1919-04-10） |

## How it is built

The formatters take the language and the typesetting as two arguments today, threaded through five functions. They become one small value (language, typesetting, date style) resolved once per page where the typesetting already is, so the new choice adds no parameter to any caller and the rule "vertical Chinese keeps its numerals" stays in the one predicate that owns it. The values are a closed list, so a later style (a numeric local form such as 10.04.1919) is one more row. There is no page-level key and no cascade until a site needs one.

A moss from before this setting reads a config that has it, warns that `[site]` has an unknown key it does not read, and builds the written form. The site still builds and publishes.

## Tests

- Each formatter, in the four languages, at full, month and year precision, under both values.
- A vertical Chinese page under `iso` keeps its numerals and the build notes it once.
- A built site under `iso`: the article line, a listing, a year-grouped list, a comment and an event read as the table says, and every `datetime` attribute and the feed are byte-identical to the `written` build.
- An unknown value warns and falls back to `written`.
- A comment added in the browser and the reader's-own-day stamp follow the page's `data-date-style`.
