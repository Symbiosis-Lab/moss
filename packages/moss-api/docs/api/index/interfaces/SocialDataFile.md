[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / SocialDataFile

# Interface: SocialDataFile

A `.moss/data/social/*.json` file: the schema version plus every article's
social data, keyed by article path.

## Properties

### articles

```ts
articles: Record<string, SocialArticleData>;
```

***

### schemaVersion

```ts
schemaVersion: string;
```
