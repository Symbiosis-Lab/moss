[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DownloadResult

# Interface: DownloadResult

Result from an asset download operation

## Properties

### actualPath

```ts
actualPath: string;
```

Actual path where file was saved (relative to project)

***

### bytesWritten

```ts
bytesWritten: number;
```

Number of bytes written to disk

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
