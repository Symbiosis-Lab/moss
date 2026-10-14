[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DerivationContribution

# Interface: DerivationContribution

`contributes.derivations[id]`: a file type the plugin turns into embeddable
fragments by running a pinned one-shot binary.

## Properties

### accepts

```ts
accepts: string[];
```

Lowercase file extensions without the dot.

***

### fragments

```ts
fragments: Record<string, "markdown">;
```

Fragments the binary may produce, by name. Only "markdown" exists.

***

### noun

```ts
noun: object;
```

#### one

```ts
one: string;
```

#### other

```ts
other: string;
```

***

### run

```ts
run: object;
```

#### artifact

```ts
artifact: ArtifactSource[];
```

#### binary

```ts
binary: string;
```

***

### stub?

```ts
optional stub?: string[];
```

Placeholder lines: each is empty or `#<fragment name>`.
