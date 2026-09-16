[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockCookieStorage

# Interface: MockCookieStorage

Mock cookie storage for plugin authentication testing

## Properties

### cookies

```ts
cookies: Map<string, object[]>;
```

Map of pluginName:projectPath to cookies

## Methods

### clear()

```ts
clear(): void;
```

Clear all cookies

#### Returns

`void`

***

### clearCookies()

```ts
clearCookies(pluginName, projectPath): void;
```

Remove every cookie stored for one plugin/project

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `projectPath` | `string` |

#### Returns

`void`

***

### getCookies()

```ts
getCookies(pluginName, projectPath): object[];
```

Get cookies for a plugin/project

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `projectPath` | `string` |

#### Returns

`object`[]

***

### setCookies()

```ts
setCookies(
   pluginName, 
   projectPath, 
   cookies): void;
```

Set cookies for a plugin/project

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `projectPath` | `string` |
| `cookies` | `object`[] |

#### Returns

`void`
