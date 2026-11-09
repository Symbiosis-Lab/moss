[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DeployContext

# Interface: DeployContext

Context for the deploy hook (deployer plugins)

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

### index\_file?

```ts
optional index_file?: string;
```

Absolute path beside `site_dir` to pass to git as `GIT_INDEX_FILE`, so
staging `site_dir` leaves the project's own index alone. Not created
until git writes it; removed with `site_dir`. Set from moss 0.15.20.

***

### project\_info

```ts
project_info: ProjectInfo;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`project_info`](BaseContext.md#project_info)

***

### site\_dir?

```ts
optional site_dir?: string;
```

Absolute path of a read-only directory holding exactly `site_files`,
made for this hook call and removed when it returns. Read the site here
(pass it as `executeBinary`'s `workingDir`, or to `tar -C`), not through
`readSiteFile`, whose tree the next build can advance mid-publish.
Set from moss 0.15.20: a plugin that reads it declares
`min_moss_version` "0.15.20", and should stop if it is missing rather
than fall back to the project folder.

***

### site\_files

```ts
site_files: string[];
```

The files to deploy, relative to `site_dir`.
