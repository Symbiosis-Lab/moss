[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / SyndicateContext

# Interface: SyndicateContext

Context for after_deploy hook (syndicator plugins)

## Extends

- [`BaseContext`](BaseContext.md)

## Properties

### articles

```ts
articles: ArticleInfo[];
```

***

### config

```ts
config: Record<string, unknown>;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`config`](BaseContext.md#config)

***

### deployment?

```ts
optional deployment?: DeploymentInfo;
```

***

### project\_info

```ts
project_info: ProjectInfo;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`project_info`](BaseContext.md#project_info)

***

### site\_files

```ts
site_files: string[];
```
