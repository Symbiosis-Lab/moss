[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / FetchResult

# Interface: FetchResult

Result from an HTTP fetch operation

## Properties

### body

```ts
body: Uint8Array;
```

Response body as Uint8Array

***

### contentType

```ts
contentType: string | null;
```

Content-Type header from response

***

### ok

```ts
ok: boolean;
```

Whether the request was successful (2xx status)

***

### status

```ts
status: number;
```

HTTP status code

## Methods

### text()

```ts
text(): string;
```

Get response body as text

#### Returns

`string`
