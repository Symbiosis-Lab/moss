[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / CopyJob

# Interface: CopyJob

The one copy a channel's `publish_copy`, `update_copy` or `find_copy`
receives, already shaped to the limits its manifest declares
under `contributes.channel.syndication`.

## Properties

### article?

```ts
optional article?: 
  | {
  blocks?: ArticleBlock[];
  cover?: string | null;
  description?: string | null;
  html: string;
  text?: string;
  title: string;
}
  | null;
```

#### Union Members

##### Type Literal

```ts
{
  blocks?: ArticleBlock[];
  cover?: string | null;
  description?: string | null;
  html: string;
  text?: string;
  title: string;
}
```

##### blocks?

```ts
optional blocks?: ArticleBlock[];
```

`html` as an editor is given it, split at its pictures in reading order:
head elements, a leading h1 repeating the title and footnote back-links
removed, embeds as links, a heading over the notes. Formula pictures stay inline.

##### cover?

```ts
optional cover?: string | null;
```

##### description?

```ts
optional description?: string | null;
```

##### html

```ts
html: string;
```

##### text?

```ts
optional text?: string;
```

The page as written, in plain text: no title and no link back.

##### title

```ts
title: string;
```

***

`null`

***

### channel

```ts
channel: string;
```

***

### config

```ts
config: Record<string, unknown>;
```

The channel's resolved settings.

***

### form?

```ts
optional form?: "short" | "thread" | "long" | "teaser";
```

Payload fields may be absent for a lookup whose copy no longer fits.

***

### idempotency\_key

```ts
idempotency_key: string;
```

The same on every send of this post to this channel, on every computer.

***

### included\_pictures?

```ts
optional included_pictures?: number;
```

Included HTML image occurrences for long articles; attachment entries for other forms.

***

### omitted\_pictures?

```ts
optional omitted_pictures?: number;
```

Pictures omitted by the channel's image limit.

***

### op

```ts
op: "publish" | "update" | "find";
```

***

### parts?

```ts
optional parts?: CopyPart[];
```

***

### pictures?

```ts
optional pictures?: CopyPicture[];
```

Upload attachments for plain copies. Long articles own images in HTML, with no duplicate attachments.

***

### post

```ts
post: object;
```

The original the copy points back to; `url` is absolute, `title` empty for an untitled post.

#### date?

```ts
optional date?: string | null;
```

#### lang?

```ts
optional lang?: string | null;
```

#### source\_path

```ts
source_path: string;
```

#### tags?

```ts
optional tags?: string[];
```

#### title

```ts
title: string;
```

#### url

```ts
url: string;
```

***

### previous?

```ts
optional previous?: CopyRef & object | null;
```

The copy already on the network (for an update or lookup), or the draft a hand-off left there.

***

### site

```ts
site: object;
```

The site the post lives on: `post.url` is built on `url` (absolute, no trailing slash); `name` falls back to its host.
`icon` is the absolute URL of the largest square raster of at least 256 px, present only when the site has its own icon.

#### icon?

```ts
optional icon?: string;
```

#### name

```ts
name: string;
```

#### url

```ts
url: string;
```

***

### teaser?

```ts
optional teaser?: 
  | {
  cover?: string | null;
  description: string;
  title: string;
  url: string;
}
  | null;
```
