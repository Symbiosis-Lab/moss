[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ListingResult

# Interface: ListingResult

What the `listing` export returns. A malformed result, a throw or a timeout
leaves moss's own listing in place with one build warning.

## Properties

### content\_width?

```ts
optional content_width?: "wide" | "full";
```

Overrides the style's declared `content_width` for this page.

***

### files?

```ts
optional files?: (
  | {
  path: string;
  text: string;
}
  | {
  base64: string;
  path: string;
})[];
```

Files written into the build output inside the listing page's folder.
A path that is absolute, contains `..`, starts with `_moss/` or collides
with a page is refused with a warning.

***

### head?

```ts
optional head?: (
  | {
  stylesheet: string;
}
  | {
  script: string;
})[];
```

Tags added to the listing page's `<head>`. A script is emitted with `defer`, so it runs after the page, listing included, is parsed.

***

### html

```ts
html: string;
```

Replaces the listing where moss would have drawn it.

***

### warnings?

```ts
optional warnings?: (
  | string
  | {
  page?: string;
  text: string;
})[];
```

Build warnings, prefixed with the plugin's name (`--strict` fails on them).
`{ text, page }` names a record's `url_path`; the warning then names that page's source file.
