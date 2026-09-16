[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / MultipartTextField

# Interface: MultipartTextField

One ordered text field in a multipart/form-data POST.

Order is preserved because the GraphQL multipart request spec requires
`operations` before `map` before the file parts.

## Properties

### name

```ts
name: string;
```

Form field name (e.g. "operations", "map").

***

### value

```ts
value: string;
```

Field value (e.g. the JSON-encoded GraphQL operation).
