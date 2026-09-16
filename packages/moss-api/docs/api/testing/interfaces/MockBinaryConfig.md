[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockBinaryConfig

# Interface: MockBinaryConfig

Configuration for mocking binary execution

## Properties

### defaultResult

```ts
defaultResult: MockBinaryResult;
```

Default result for unregistered binaries

***

### results

```ts
results: Map<string, MockBinaryResult>;
```

Map of binary commands to results

## Methods

### getResult()

```ts
getResult(binaryPath, args): MockBinaryResult;
```

Get result for a binary command

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `binaryPath` | `string` |
| `args` | `string`[] |

#### Returns

[`MockBinaryResult`](MockBinaryResult.md)

***

### reset()

```ts
reset(): void;
```

Reset all configurations

#### Returns

`void`

***

### setResult()

```ts
setResult(key, result): void;
```

Set result for a binary command (key format: "binaryPath args...")

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `key` | `string` |
| `result` | [`MockBinaryResult`](MockBinaryResult.md) |

#### Returns

`void`
