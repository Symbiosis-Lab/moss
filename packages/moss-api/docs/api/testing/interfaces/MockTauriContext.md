[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockTauriContext

# Interface: MockTauriContext

Context returned by setupMockTauri with all mock utilities

## Properties

### binaryConfig

```ts
binaryConfig: MockBinaryConfig;
```

Binary execution configuration

***

### browserTracker

```ts
browserTracker: MockBrowserTracker;
```

Browser open/close tracking

***

### cleanup

```ts
cleanup: () => void;
```

Cleanup function - must be called after tests

#### Returns

`void`

***

### cookieStorage

```ts
cookieStorage: MockCookieStorage;
```

Cookie storage

***

### dialogTracker

```ts
dialogTracker: MockDialogTracker;
```

Dialog interaction tracking

***

### downloadTracker

```ts
downloadTracker: DownloadTracker;
```

Download tracking for concurrency tests

***

### filesystem

```ts
filesystem: MockFilesystem;
```

In-memory filesystem

***

### pluginName

```ts
pluginName: string;
```

The plugin name used for internal context

***

### projectPath

```ts
projectPath: string;
```

The project path used for internal context

***

### urlConfig

```ts
urlConfig: MockUrlConfig;
```

URL response configuration
