[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / TauriCore

# Interface: TauriCore

Tauri core utilities for plugin communication

## Properties

### invoke

```ts
invoke: <T>(cmd, args?) => Promise<T>;
```

#### Type Parameters

| Type Parameter |
| ------ |
| `T` |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `cmd` | `string` |
| `args?` | `Record`\<`string`, `unknown`\> |

#### Returns

`Promise`\<`T`\>
