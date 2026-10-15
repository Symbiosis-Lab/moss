[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ListingStyleContribution

# Interface: ListingStyleContribution

`contributes.listing_styles[id]`: a way of listing a folder that the
plugin's `listing` export draws. An author picks it with
`children_style: <id>` on the folder's home page.

## Properties

### content\_width?

```ts
optional content_width?: "wide" | "full";
```

The page width this style wants; applies to a page that sets none.

***

### description?

```ts
optional description?: string;
```

One sentence for the select's hint.

***

### label?

```ts
optional label?: string;
```

Name shown in the editor's select.
