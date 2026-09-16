[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ProcessContext

# Interface: ProcessContext

Context for before_build hook (process capability)

`trigger` is stamped by moss (ADR-015): the plugin reads it to declare task
intent via `startTask`, it does NOT guess it. Onboarding card → "onboarding_flow"
(drives the ambient hairline); every build/preview rebuild → "background".
Optional for backward compatibility; absent ⇒ treat as "background".

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

### trigger?

```ts
optional trigger?: TriggerContext;
```
