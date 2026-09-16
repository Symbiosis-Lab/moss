[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ConfigureDomainContext

# Interface: ConfigureDomainContext

Context for configure_domain hook (custom domain setup on deploy platform)

Called after DNS records are configured via moss-oracle. Allows deploy plugins
to perform platform-specific domain setup (e.g., GitHub Pages CNAME configuration).

This is NOT a separate capability - it's an optional hook on Deploy-capable plugins.

## Idempotency Contract

The domain orchestrator calls this hook at multiple lifecycle points:
1. After DNS records are configured (site may not be live yet)
2. After the site is verified live via HTTP 200

**Plugins MUST implement this hook as idempotent.** The plugin should:
- Check current platform state
- Do only the next needed step
- Return success as a no-op if already fully configured

Example (GitHub Pages):
- Call 1: Sets CNAME file via API
- Call 2: Verifies CNAME is set, enforces HTTPS via API
- Call 3+: Both already done, returns success without changes

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

### deployment

```ts
deployment: DeploymentInfo;
```

Deployment information from the last deploy

***

### domain

```ts
domain: string;
```

The custom domain being configured (e.g., "example.com")

***

### project\_info

```ts
project_info: ProjectInfo;
```

#### Inherited from

[`BaseContext`](BaseContext.md).[`project_info`](BaseContext.md#project_info)
