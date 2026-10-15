[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / PageRecord

# Interface: PageRecord

One page, as a listing plugin sees it. The listing page itself, every page
it lists and every subfolder's home page have this shape.

## Properties

### field\_errors

```ts
field_errors: Record<string, string>;
```

For each declared field whose value does not fit, the reason it does not, as
the build warning states it. Empty when every value fits.

***

### fields

```ts
fields: Record<string, unknown>;
```

The page's declared fields (`[fields.*]` in `.moss/config.toml`) that its own
frontmatter carries and that parse, each as its typed value. A date is
`{ value, precision, qualifier }`; a period is `{ start, end, open, text }` with
`start` and `end` in that date shape and `open` true only for an interval with
no end; a choice is `{ value, label }` with the label in this page's language;
a count is `{ number, noun, text }`; a page link is `{ target, alias? }`, with `alias` the text after `|` in `[[target|alias]]` when there is one. A value that does not parse is left out
here and stays in `frontmatter`. Values a folder's `cascade:` supplies are not
typed here.

***

### folder

```ts
folder: string;
```

The folder the page sits in, relative to the listed folder: empty for the
listing page and for its direct children.

***

### frontmatter

```ts
frontmatter: Record<string, unknown>;
```

The page's frontmatter as moss resolved it: values inherited from a
parent folder are applied and `title` is filled in when the page set none.

***

### html

```ts
html: string;
```

The rendered body. Relative links and image sources are rewritten to
resolve from the listing page, where the HTML is drawn. Empty for a
subfolder that has no home page.

***

### lang

```ts
lang: string;
```

***

### title

```ts
title: string;
```

***

### translations

```ts
translations: object[];
```

#### lang

```ts
lang: string;
```

#### url\_path

```ts
url_path: string;
```

***

### url

```ts
url: string;
```

Where the page is, written so it resolves from the listing page.

***

### url\_path

```ts
url_path: string;
```

The page's address in the built site, as moss names it (`board/index.html`).
