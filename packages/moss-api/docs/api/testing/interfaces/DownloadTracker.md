[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / DownloadTracker

# Interface: DownloadTracker

Tracks download activity for testing concurrency and completion

## Properties

### activeDownloads

```ts
activeDownloads: number;
```

Number of currently active downloads

***

### completedDownloads

```ts
completedDownloads: string[];
```

URLs of completed downloads

***

### failedDownloads

```ts
failedDownloads: object[];
```

Failed downloads with error messages

#### error

```ts
error: string;
```

#### url

```ts
url: string;
```

***

### maxConcurrent

```ts
maxConcurrent: number;
```

Maximum concurrent downloads observed

## Methods

### endDownload()

```ts
endDownload(
   url, 
   success, 
   error?): void;
```

Mark a download as ended

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `url` | `string` |
| `success` | `boolean` |
| `error?` | `string` |

#### Returns

`void`

***

### reset()

```ts
reset(): void;
```

Reset all tracking state

#### Returns

`void`

***

### startDownload()

```ts
startDownload(url): void;
```

Mark a download as started

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `url` | `string` |

#### Returns

`void`
