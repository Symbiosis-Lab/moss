[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ListingContext

# Interface: ListingContext

The argument of the `listing` export.

`model` is the version of this shape. Fields may be added within a
version; renaming or removing one bumps it.

## Properties

### folders

```ts
folders: PageRecord[];
```

The listed folder's subfolders that have a home page, in the listing's own order (its sort applied to the home pages).

***

### model

```ts
model: 1;
```

***

### page

```ts
page: PageRecord;
```

***

### pages

```ts
pages: PageRecord[];
```

Exactly the pages moss's own listing of the folder would show, in the order it shows them.

***

### style

```ts
style: string;
```

The style id the page asked for.

***

### terms

```ts
terms: Record<string, Record<string, string>>;
```

`terms[namespace][name]` is where that term's page is, written like a record's `url` so it resolves from the listing page, for every name `pages` use.
