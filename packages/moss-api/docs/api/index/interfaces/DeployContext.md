[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DeployContext

# Interface: DeployContext

Context for on_deploy hook (deployer plugins)

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

### domain?

```ts
optional domain?: string;
```

Custom domain from .moss/config.toml [deployment] section (if configured)

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
