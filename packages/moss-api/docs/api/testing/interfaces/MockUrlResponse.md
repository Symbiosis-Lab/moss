[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockUrlResponse

# Interface: MockUrlResponse

Configuration for a mocked URL response

## Properties

### actualPath?

```ts
optional actualPath?: string;
```

Actual file path where asset was saved

***

### bodyBase64?

```ts
optional bodyBase64?: string;
```

Response body as base64 (for fetch_url)

***

### bytesWritten?

```ts
optional bytesWritten?: number;
```

Number of bytes written (for download_asset)

***

### contentType?

```ts
optional contentType?: string;
```

Content-Type header

***

### delay?

```ts
optional delay?: number;
```

Artificial delay in milliseconds

***

### ok

```ts
ok: boolean;
```

Whether the request was successful (2xx)

***

### status

```ts
status: number;
```

HTTP status code
