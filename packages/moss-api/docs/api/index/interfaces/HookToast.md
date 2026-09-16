[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / HookToast

# Interface: HookToast

Outcome notification described by a hook result.

This is data about what happened, not a rendering instruction: moss owns
every status surface (per its plugin-architecture boundary) and maps
`outcome` to its own toast severity, timing, and suppression rules — e.g.
a surface that already shows the outcome (the first-publish wizard) can
swallow it entirely.

## Properties

### outcome

```ts
outcome: "error" | "success" | "info";
```

What happened: "success" | "info" | "error"

***

### title

```ts
title: string;
```

Short display text (e.g., "Live on Tor", "No changes to deploy")

***

### url?

```ts
optional url?: string | null;
```

Optional clickable URL (e.g., the deployed site URL)
