[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / SetupContext

# Interface: SetupContext

Context for the optional `check_setup` hook (deploy plugins).

Deliberately not a [BaseContext](BaseContext.md): this runs on the Publish click,
before the build, so it carries no project scan — only where the folder is,
the plugin's resolved settings, and what the user just submitted.

`action` is what makes the probe a conversation rather than a verdict. The
first call arrives without one; submitting a blocker's form calls the same
hook again with that blocker's `id` as `action` and the form's `values`,
and the plugin does the work and answers with the next state. Every call is
cold — re-derive the current step from durable state, never from memory.

```typescript
export async function check_setup(ctx: SetupContext): Promise<HookResult> {
  if (ctx.action === "start_daemon") await startDaemon();
  if (await daemonIsUp()) return { success: true, setup: { status: "ready" } };
  return {
    success: true,
    setup: {
      status: "blocked",
      blockers: [{
        id: "start_daemon",
        message:
          "The IPFS daemon isn't running. It keeps running after moss quits.",
        form: { fields: [], submit: "Start it" },
      }],
    },
  };
}
```

## Properties

### action?

```ts
optional action?: string;
```

The id of the blocker whose form was submitted, absent on the first call

***

### ~~config~~

```ts
config: Record<string, unknown>;
```

#### Deprecated

The same map as [SetupContext.settings](#settings), pre-contract name.

***

### project\_path

```ts
project_path: string;
```

Absolute path to the project folder about to be published

***

### settings

```ts
settings: Record<string, unknown>;
```

The plugin's resolved plain settings (declared defaults ∪ saved values)

***

### values?

```ts
optional values?: Record<string, unknown>;
```

The submitted form values riding with `action`; empty for a button
