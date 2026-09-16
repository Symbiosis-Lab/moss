[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / MultipartFilePart

# Interface: MultipartFilePart

One file part in a multipart/form-data POST. Bytes are passed base64-encoded —
e.g. straight from `readSiteFile`, which already returns base64.

## Properties

### contentBase64

```ts
contentBase64: string;
```

File contents, base64-encoded.

***

### contentType

```ts
contentType: string;
```

MIME type for the part's Content-Type header.

***

### field

```ts
field: string;
```

Form field name for this file (e.g. "0" per the GraphQL multipart spec).

***

### filename

```ts
filename: string;
```

File name reported in the part's Content-Disposition.
