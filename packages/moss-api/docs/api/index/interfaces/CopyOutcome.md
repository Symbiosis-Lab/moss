[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / CopyOutcome

# Interface: CopyOutcome

A copy hook's answer, in [HookResult.copy](HookResult.md#copy): the copy's id and address;
or, with `success: false`, why the network refused; or a hand-off, a copy
the author posts by hand (`text` to paste at `open_url`) or a draft on the
network for the author to publish (empty `text`, the draft's `id`).

## Properties

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
