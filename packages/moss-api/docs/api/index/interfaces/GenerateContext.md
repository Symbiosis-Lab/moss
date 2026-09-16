[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / GenerateContext

# Interface: GenerateContext

Context for on_build hook (generator plugins)

## Extends

- [`BaseContext`](BaseContext.md)

## Properties

### config

```ts
config: Record<string, unknown>;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`config`](BaseContext.md#config)

***

### page\_tree?

```ts
optional page_tree?: PageNode;
```

Resolved Page Tree — universal content model for all generators

***

### project\_info

```ts
project_info: ProjectInfo;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`project_info`](BaseContext.md#project_info)

***

### source\_files

```ts
source_files: SourceFiles;
```
