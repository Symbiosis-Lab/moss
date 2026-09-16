[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / AdvisoryProposal

# Interface: AdvisoryProposal

A plugin's PROPOSED advisory (pre-clamp). Mirrors the Rust
`plugins::types::PluginAdvisory` wire shape exactly so it deserializes
directly into `PluginTaskLifecycle::Succeeded/Failed { advisories }`. moss
is the only constructor of a final `Advisory` — a plugin can never hand moss
one (R13).

## Properties

### action

```ts
action: AdvisoryAction;
```

The recovery affordance the plugin proposes.

***

### item

```ts
item: string | null;
```

The item this is about — usually a filename. `null` for build-wide.

***

### scope

```ts
scope: AdvisoryScope;
```

Which axis of the system this advisory is about.

***

### severity

```ts
severity: AdvisorySeverity;
```

The severity the plugin REQUESTS. moss clamps it (R13).

***

### what

```ts
what: string;
```

What happened (free text).
