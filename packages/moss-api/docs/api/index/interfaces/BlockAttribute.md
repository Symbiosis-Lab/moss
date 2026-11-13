[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / BlockAttribute

# Interface: BlockAttribute

One attribute a block's fence may set. Only what the fence wrote is
emitted, so an attribute has no default.

## Properties

### key

```ts
key: string;
```

***

### options?

```ts
optional options?: string[];
```

The only values a `string` attribute takes.

***

### pattern?

```ts
optional pattern?: string;
```

A regular expression the value must contain a match of; anchor it with `^…$` to hold the whole value.

***

### pattern\_message?

```ts
optional pattern_message?: string;
```

What the warning says when `pattern` does not match; optional.

***

### required?

```ts
optional required?: boolean;
```

A fence that leaves it out is reported.

***

### type?

```ts
optional type?: "string" | "number" | "boolean";
```

Defaults to `"string"`.
