[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / SocialComment

# Interface: SocialComment

One comment in the .moss/data/social/*.json shared standard.
 See moss/docs/reference/social-data-standard.md.

## Properties

### author

```ts
author: object;
```

#### displayName

```ts
displayName: string;
```

#### url?

```ts
optional url?: string;
```

***

### content

```ts
content: string;
```

***

### createdAt

```ts
createdAt: string;
```

***

### id

```ts
id: string;
```

***

### replyToId?

```ts
optional replyToId?: string;
```

***

### source

```ts
source: string;
```

***

### state?

```ts
optional state?: string;
```

Moderation state. Absent or "active" = visible; anything else is filtered
at read time. Well-known values: "active" | "removed" | "archived" |
"banned" | "collapsed". The Rust side treats this as an open string so
future states do not require a client update.
