[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / CopyJob

# Interface: CopyJob

The one copy a channel's `publish_copy`, `update_copy`, `delete_copy` or
`find_copy` receives, already shaped to the limits its manifest declares
under `contributes.channel.syndication`.

## Properties

### article?

```ts
optional article?: 
  | {
  cover?: string | null;
  description?: string | null;
  html: string;
  title: string;
}
  | null;
```

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
optional form?: "short" | "thread" | "long" | "teaser" | "handoff";
```

Absent for a delete, whose page is gone.

***

### idempotency\_key

```ts
idempotency_key: string;
```

The same on every send of this post to this channel, on every computer.

***

### op

```ts
op: "publish" | "update" | "delete" | "find";
```

***

### parts?

```ts
optional parts?: CopyPart[];
```

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

The copy already on the network (for an update, delete or lookup), or the draft a hand-off left there.

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
