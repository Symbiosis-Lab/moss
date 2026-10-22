[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockFile

# Interface: MockFile

A file stored in the mock filesystem

## Properties

### bytes?

```ts
optional bytes?: Uint8Array<ArrayBufferLike>;
```

Exact bytes, set by binary writes (`writeFileBinary`, `downloadAsset`); `content` is then their lossy UTF-8 reading.

***

### content

```ts
content: string;
```

***

### createdAt

```ts
createdAt: Date;
```

***

### modifiedAt

```ts
modifiedAt: Date;
```
