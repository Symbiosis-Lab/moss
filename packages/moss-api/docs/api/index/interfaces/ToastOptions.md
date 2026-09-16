[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ToastOptions

# Interface: ToastOptions

Options for showing a toast notification

## Example

```typescript
// Simple success toast
showToast({ message: "Saved!" });

// Toast with action
showToast({
  message: "Deployed!",
  variant: "success",
  actions: [{ label: "View site", url: "https://..." }],
  duration: 8000
});

// Persistent progress toast
showToast({
  message: "Deploying...",
  variant: "info",
  id: "deploy-progress",
  persistent: true,
  dismissible: false
});
```

## Properties

### actions?

```ts
optional actions?: ToastAction[];
```

Action buttons (opens URL in system browser)

***

### dismissible?

```ts
optional dismissible?: boolean;
```

If false, hide the X close button (default: true)

***

### duration?

```ts
optional duration?: number;
```

Duration in ms before auto-dismiss (bounded by frontend to 2000-30000)

***

### id?

```ts
optional id?: string;
```

Unique ID for update-in-place pattern (used with updateToast/dismissToast)

***

### message

```ts
message: string;
```

The message to display (required)

***

### persistent?

```ts
optional persistent?: boolean;
```

If true, toast stays until user dismisses or plugin calls dismissToast()

***

### variant?

```ts
optional variant?: ToastVariant;
```

Visual style hint (default: "info")
