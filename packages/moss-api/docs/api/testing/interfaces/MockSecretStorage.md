[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockSecretStorage

# Interface: MockSecretStorage

Mock secret storage.

Scoped by plugin, as the host scopes it — a mock keyed only by the secret's
name would let a test pass that the host would refuse.

`seed` stands in for the user answering moss's modal; a plugin's own
`setSecret` writes the same store. `rejectSecret` re-asks, so the mock answers
for the user — it cancels unless a test called `answerNextPrompt`, because
cancel is the case plugins forget to handle.

## Methods

### answerNextPrompt()

```ts
answerNextPrompt(value): void;
```

What the next re-prompt answers. Consumed by one `reject`, then cancel again.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `value` | `string` \| `null` |

#### Returns

`void`

***

### clear()

```ts
clear(): void;
```

#### Returns

`void`

***

### get()

```ts
get(pluginName, key): string | null;
```

What `getSecret` would return: the stored value, or `null`.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `key` | `string` |

#### Returns

`string` \| `null`

***

### reject()

```ts
reject(pluginName, key): string | null;
```

Forget it, re-ask, and answer — what `rejectSecret` does end to end.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `key` | `string` |

#### Returns

`string` \| `null`

***

### seed()

```ts
seed(
   pluginName, 
   key, 
   value): void;
```

Store a secret as if the user had answered moss's credential modal.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `key` | `string` |
| `value` | `string` |

#### Returns

`void`

***

### wasRejected()

```ts
wasRejected(pluginName, key): boolean;
```

Did the plugin tell moss to forget this secret?

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `pluginName` | `string` |
| `key` | `string` |

#### Returns

`boolean`
