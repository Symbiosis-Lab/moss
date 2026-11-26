[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / EditorFill

# Interface: EditorFill

What [fillEditor](../README.md#filleditor) puts where: the editor's elements on the network's
page, the article in parts, and which of moss's reshapings the editor needs.

## Properties

### editor

```ts
editor: object;
```

#### body

```ts
body: string;
```

The editable body.

#### ready?

```ts
optional ready?: string;
```

Present once the editor has loaded; `body` when absent.

#### title?

```ts
optional title?: string;
```

The title field, an input, a textarea or an editable element.

#### upload?

```ts
optional upload?: string;
```

A file field (`input[type=file]`) for a network whose post starts from
pictures: they are given to it first, as many as it allows and of the
kinds it accepts, then the editor is filled once it appears. Picture parts go
to the field, not the editor. No such field answers `no-editor`; a field
that takes none of them, or an editor that never appears after them,
`refused`.

***

### parts

```ts
parts: EditorFillPart[];
```

***

### picture\_ms?

```ts
optional picture_ms?: number;
```

How long each picture may take to appear. Default 15 s.

***

### rules?

```ts
optional rules?: object;
```

#### code?

```ts
optional code?: "paragraphs";
```

One paragraph per line of code.

#### headings?

```ts
optional headings?: "two-levels";
```

The article's top heading level becomes h1, every deeper one h2.

#### tables?

```ts
optional tables?: "rows";
```

One paragraph per table row, cells joined by " · ".

***

### timeout\_ms?

```ts
optional timeout_ms?: number;
```

How long to wait for the editor. Default 20 s.

***

### title?

```ts
optional title?: string;
```
