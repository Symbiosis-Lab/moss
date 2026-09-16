[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / SetupField

# Interface: SetupField

One field of a [SetupForm](SetupForm.md) — the same vocabulary a manifest's
`settings[]` declares, minus `when` (in a stepped flow, the steps are the
conditionality). moss draws it; the plugin supplies no pixels.

## Properties

### default?

```ts
optional default?: unknown;
```

A suggestion moss pre-fills — a claimed-name candidate, say

***

### description?

```ts
optional description?: string;
```

One sentence under the field

***

### help\_url?

```ts
optional help_url?: string;
```

***

### key

```ts
key: string;
```

The key the submitted value arrives under in `SetupContext.values`

***

### label?

```ts
optional label?: string;
```

***

### options?

```ts
optional options?: object[];
```

Present on a `string` field, it closes the value set: moss draws a select

#### description?

```ts
optional description?: string;
```

#### label

```ts
label: string;
```

#### value

```ts
value: string;
```

***

### pattern?

```ts
optional pattern?: string;
```

Checked per keystroke; requires `pattern_message`

***

### pattern\_message?

```ts
optional pattern_message?: string;
```

***

### placeholder?

```ts
optional placeholder?: string;
```

***

### type

```ts
type: "string" | "number" | "boolean" | "secret";
```
