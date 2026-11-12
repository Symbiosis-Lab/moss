[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / PublicDataContribution

# Interface: PublicDataContribution

`contributes.public_data`: the part of the site's settings a plugin's
element reads in the browser, published at
`/_moss/extensions/<plugin>.json` as `{schema, data}`.

## Properties

### fill?

```ts
optional fill?: Record<string, {
  path: string;
  service: string;
}>;
```

Keys the build fills with the address of one of moss's services when the table leaves them out.

***

### keys

```ts
keys: string[];
```

Dotted paths below the plugin's own `[services.<plugin>]` table, `*`
standing for any one key: `cancel.late_percent`, `services.*.name`. Each
ends at a value that is not a table; nothing else is published.
