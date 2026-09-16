[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / StartTaskOptions

# Interface: StartTaskOptions

## Properties

### cancellable?

```ts
optional cancellable?: boolean;
```

Whether the user can cancel this task from the UI. Cancellation
plumbing lands in a later ADR phase; the flag is recorded today so
renderers can show / hide a cancel affordance.

***

### hasProgress?

```ts
optional hasProgress?: boolean;
```

Hint to the router that this task will emit `progress()` updates
with fractions. Routers and renderers MAY use this to choose between
"fills" vs "pulses" visualizations. Defaults to true.

***

### hook?

```ts
optional hook?: PluginHook;
```

Hook the task belongs to. Defaults to "import" — the most common
onboarding case. Plugins should pass an explicit hook for non-import
work (e.g., a syndicator passes "syndicate").

***

### job?

```ts
optional job?: string;
```

Plugin-local job id referencing `contributes.jobs[id]` in the plugin's
manifest (Step 3 Phase 5, §8 + R13). When set, moss looks up the declared
descriptor (a past-tense `verb` + an amount `noun`), normalizes the verb
(`Verb::normalized` — moss owns capitalization/length/glyphs), and on a
`succeeded(receipt, count)` stamps its OWN typed `Verb` + `Amount { count,
noun }` on the Job — rendering "Syndicated · N posts" from moss's value
objects, never the plugin's pre-formatted `receipt` string. Omit it for
free-text-receipt tasks (the legacy path, byte-identical).

***

### trigger?

```ts
optional trigger?: TriggerContext;
```

Trigger context. Defaults to "background" — the safest fallback
because Background routes to Workspace+Ambient, the quietest surface.
Plugins running inside the onboarding flow should pass
"onboarding_flow" explicitly so they reach the ActionPanel hairline.
