[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / BaseContext

# Interface: BaseContext

Base context shared by all hooks

Contains only business data - no paths.
Use readFile(), writeFile() for project files.
Use readPluginFile(), writePluginFile() for plugin storage.

## Extended by

- [`ProcessContext`](ProcessContext.md)
- [`GenerateContext`](GenerateContext.md)
- [`DeployContext`](DeployContext.md)
- [`ConfigureDomainContext`](ConfigureDomainContext.md)
- [`SyndicateContext`](SyndicateContext.md)

## Properties

### config

```ts
config: Record<string, unknown>;
```

***

### project\_info

```ts
project_info: ProjectInfo;
```
