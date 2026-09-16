[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockFilesystem

# Interface: MockFilesystem

In-memory filesystem for testing file operations

## Properties

### files

```ts
files: Map<string, MockFile>;
```

Internal file storage

## Methods

### clear()

```ts
clear(): void;
```

Clear all files

#### Returns

`void`

***

### deleteFile()

```ts
deleteFile(path): boolean;
```

Delete a file

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |

#### Returns

`boolean`

***

### getFile()

```ts
getFile(path): MockFile | undefined;
```

Get a file by full path

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |

#### Returns

[`MockFile`](MockFile.md) \| `undefined`

***

### listFiles()

```ts
listFiles(pattern?): string[];
```

List files matching an optional pattern

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pattern?` | `string` |

#### Returns

`string`[]

***

### setFile()

```ts
setFile(path, content): void;
```

Set a file's content (creates or updates)

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |
| `content` | `string` |

#### Returns

`void`
