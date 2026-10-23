[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ProcessContext

# Interface: ProcessContext

Context for before_build hook (process capability)

`trigger` is stamped by moss: the plugin reads it to declare task
intent via `startTask`, it does NOT guess it. Onboarding card → "onboarding_flow"
(drives the ambient hairline); every build/preview rebuild → "background".
Optional for backward compatibility; absent ⇒ treat as "background".

`site_config` is the site's `.moss/config.toml` as a plain JSON object, so a
plugin can read tables it documents (for example `[map.layers.<name>]`).
It is read-only and `{}` when the site has no config file. Withheld: the
`[plugins]` table (your own settings arrive as `config`) and every key,
at any depth, whose name contains token, secret, password, passwd, api_key,
apikey, credential or private_key. Absent on moss versions before it was added.

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

### project\_info

```ts
project_info: ProjectInfo;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`project_info`](BaseContext.md#project_info)

***

### site\_config?

```ts
optional site_config?: Record<string, unknown>;
```

***

### trigger?

```ts
optional trigger?: TriggerContext;
```
