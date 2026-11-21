[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / EditorFillAnswer

# Interface: EditorFillAnswer

How a fill went. `refused`: the editor's content did not change after the
first paste. `no-editor`: the editor did not appear in time (its page
changed, or the panel shows a sign-in). `missing` names each picture, and
the title, that the editor did not take; `url` is where the panel is now.

## Properties

### missing

```ts
missing: string[];
```

***

### placed

```ts
placed: number;
```

***

### result

```ts
result: "filled" | "no-editor" | "refused";
```

***

### url

```ts
url: string;
```
