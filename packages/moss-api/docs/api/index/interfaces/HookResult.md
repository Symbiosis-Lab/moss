[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / HookResult

# Interface: HookResult

Standard result returned from hook execution

## Design Principles

1. **Single completion path**: Return value only, no explicit reporting
2. **Flow control only**: `success` tells moss whether to continue
3. **Outcome UX is data, not calls**: describe the outcome in `toast`;
   moss decides how (and whether) to present it. A hook must succeed with
   no UI attached at all — CLI and headless hosts run the same hooks.

## Usage Pattern

```typescript
async function deploy(context): Promise<HookResult> {
  // Do work...

  // Return result; `toast` describes the outcome for moss to render
  return {
    success: true,
    deployment: {...},
    toast: { outcome: "success", title: "Deployed!", url },
  };
}
```

## Properties

### deployment?

```ts
optional deployment?: DeploymentInfo;
```

Deployment info (populated by deploy hooks)

***

### message?

```ts
optional message?: string;
```

Detailed message for logs/debugging

***

### success

```ts
success: boolean;
```

Whether the operation succeeded

***

### toast?

```ts
optional toast?: HookToast | null;
```

Outcome notification for moss to present (moss controls rendering)
