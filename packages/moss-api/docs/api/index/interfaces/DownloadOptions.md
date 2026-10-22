[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DownloadOptions

# Interface: DownloadOptions

Options for asset download

## Properties

### filename?

```ts
optional filename?: string;
```

Name to save the file under, inside `targetDir`. One path segment: no `/`,
`\` or `..`. Default: derived from the URL's last path segment and the
response content type, so URLs that all end in `default.jpg` collide.

***

### headers?

```ts
optional headers?: Record<string, string>;
```

Extra request headers. moss sends its own `User-Agent` unless you set one.

***

### timeoutMs?

```ts
optional timeoutMs?: number;
```

Timeout in milliseconds (default: 30000)
