[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / BrowserHandle

# Interface: BrowserHandle

Handle returned by openBrowser for tracking window lifecycle

## Properties

### closed

```ts
closed: Promise<BrowserCloseReason>;
```

Promise that resolves when the browser window is closed.
Use this to detect when the user closes the window.
