[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockUrlConfig

# Interface: MockUrlConfig

URL response configuration for mocking HTTP requests

## Properties

### defaultResponse

```ts
defaultResponse: MockUrlResponse;
```

Default response for unregistered URLs

***

### responses

```ts
responses: Map<string, 
  | MockUrlResponse
| MockUrlResponse[]>;
```

Map of URL to response(s)

## Methods

### getResponse()

```ts
getResponse(url): MockUrlResponse;
```

Get response for a URL (handles retry sequences)

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `url` | `string` |

#### Returns

[`MockUrlResponse`](MockUrlResponse.md)

***

### reset()

```ts
reset(): void;
```

Reset all URL configurations

#### Returns

`void`

***

### setResponse()

```ts
setResponse(url, response): void;
```

Set response for a URL (can be single or array for retry testing)

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `url` | `string` |
| `response` | \| [`MockUrlResponse`](MockUrlResponse.md) \| [`MockUrlResponse`](MockUrlResponse.md)[] |

#### Returns

`void`
