[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / BlockContribution

# Interface: BlockContribution

`contributes.blocks[short]`: a custom element an author places with a
`:::<short>` fence around ordinary markdown. No plugin code runs at build.

## Properties

### attributes?

```ts
optional attributes?: BlockAttribute[];
```

The attributes a fence may set; anything else is dropped with a warning.

***

### element

```ts
element: string;
```

A valid custom element name.

***

### script?

```ts
optional script?: string;
```

Path inside the plugin's folder ending in `.js` or `.mjs`, loaded only on pages that use the block.

***

### stylesheet?

```ts
optional stylesheet?: string;
```

Path inside the plugin's folder ending in `.css`, loaded only on pages that use the block.
