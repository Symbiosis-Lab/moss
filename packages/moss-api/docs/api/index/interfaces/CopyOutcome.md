[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / CopyOutcome

# Interface: CopyOutcome

A copy hook's answer, in [HookResult.copy](HookResult.md#copy): the copy's id and address;
or, with `success: false`, why the network refused; or a hand-off, a copy
the author posts by hand (`text` to paste at `open_url`) or a draft on the
network for the author to publish (empty `text`, the draft's `id`).

`find_copy` answers the copy, a hand-off still waiting, or `absent: true`.
Any other answer, an empty one included, leaves whether the copy exists
unknown, and only `absent` lets moss send it again.

## Properties

### absent?

```ts
optional absent?: boolean;
```

`find_copy` only: the network says there is no such copy.

***

### account?

```ts
optional account?: string;
```

`copy_account` only: the account the channel is signed in to, as an id that stays the same while it is that account.

***

### handoff?

```ts
optional handoff?: object;
```

#### id?

```ts
optional id?: string;
```

#### open\_url?

```ts
optional open_url?: string;
```

#### text

```ts
text: string;
```

***

### id?

```ts
optional id?: string;
```

***

### parts?

```ts
optional parts?: CopyRef[];
```

A thread's later parts, in order.

***

### refused?

```ts
optional refused?: object;
```

#### cause

```ts
cause: "auth" | "content" | "rate" | "network" | "unknown";
```

#### message?

```ts
optional message?: string;
```

***

### url?

```ts
optional url?: string;
```
