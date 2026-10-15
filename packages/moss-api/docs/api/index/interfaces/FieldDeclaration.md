[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / FieldDeclaration

# Interface: FieldDeclaration

One `[fields.<name>]` declaration. `choices` is present only for a choice, in
declared order.

## Properties

### choices?

```ts
optional choices?: object[];
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

### place

```ts
place: string | null;
```

Where the field is placed on the page, or `null`.

***

### type

```ts
type: string;
```

The declared type: `text`, `number`, `date`, `url`, `choice`, `period`, `count`, `page` or `pages`.
