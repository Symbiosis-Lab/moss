[@symbiosis-lab/moss-api](../README.md) / testing

# testing

Testing utilities for moss plugins

This module provides mock implementations of Tauri IPC commands,
enabling integration testing of plugins without a running Tauri app.

## Example

```typescript
import { setupMockTauri, type MockTauriContext } from "@symbiosis-lab/moss-api/testing";
import { readFile, writeFile } from "@symbiosis-lab/moss-api";

describe("my plugin", () => {
  let ctx: MockTauriContext;

  beforeEach(() => {
    ctx = setupMockTauri();
  });

  afterEach(() => {
    ctx.cleanup();
  });

  it("reads and writes files", async () => {
    // Seed a file at the mock project root (default: /test/project)
    ctx.filesystem.setFile("/test/project/input.md", "# Hello");

    // moss-api file functions resolve paths against the project context
    const content = await readFile("input.md");
    await writeFile("output.md", content.toUpperCase());

    // Verify
    expect(ctx.filesystem.getFile("/test/project/output.md")?.content).toBe("# HELLO");
  });
});
```

## Interfaces

| Interface | Description |
| ------ | ------ |
| [DownloadTracker](interfaces/DownloadTracker.md) | Tracks download activity for testing concurrency and completion |
| [MockBinaryConfig](interfaces/MockBinaryConfig.md) | Configuration for mocking binary execution |
| [MockBinaryResult](interfaces/MockBinaryResult.md) | Result for a mocked binary execution |
| [MockBrowserTracker](interfaces/MockBrowserTracker.md) | Tracks browser open/close calls for testing |
| [MockCookieStorage](interfaces/MockCookieStorage.md) | Mock cookie storage for plugin authentication testing |
| [MockDialogResult](interfaces/MockDialogResult.md) | Dialog result types matching the moss backend |
| [MockDialogTracker](interfaces/MockDialogTracker.md) | Tracks dialog interactions for testing |
| [MockFile](interfaces/MockFile.md) | A file stored in the mock filesystem |
| [MockFilesystem](interfaces/MockFilesystem.md) | In-memory filesystem for testing file operations |
| [MockTauriContext](interfaces/MockTauriContext.md) | Context returned by setupMockTauri with all mock utilities |
| [MockUrlConfig](interfaces/MockUrlConfig.md) | URL response configuration for mocking HTTP requests |
| [MockUrlResponse](interfaces/MockUrlResponse.md) | Configuration for a mocked URL response |
| [SetupMockTauriOptions](interfaces/SetupMockTauriOptions.md) | Options for setting up mock Tauri environment |

## Functions

### createDownloadTracker()

```ts
function createDownloadTracker(): DownloadTracker;
```

Create a new download tracker instance

#### Returns

[`DownloadTracker`](interfaces/DownloadTracker.md)

***

### createMockBinaryConfig()

```ts
function createMockBinaryConfig(): MockBinaryConfig;
```

Create a new binary config instance

#### Returns

[`MockBinaryConfig`](interfaces/MockBinaryConfig.md)

***

### createMockBrowserTracker()

```ts
function createMockBrowserTracker(): MockBrowserTracker;
```

Create a new browser tracker instance

#### Returns

[`MockBrowserTracker`](interfaces/MockBrowserTracker.md)

***

### createMockCookieStorage()

```ts
function createMockCookieStorage(): MockCookieStorage;
```

Create a new cookie storage instance

#### Returns

[`MockCookieStorage`](interfaces/MockCookieStorage.md)

***

### createMockDialogTracker()

```ts
function createMockDialogTracker(): MockDialogTracker;
```

Create a new dialog tracker instance

#### Returns

[`MockDialogTracker`](interfaces/MockDialogTracker.md)

***

### createMockFilesystem()

```ts
function createMockFilesystem(): MockFilesystem;
```

Create a new mock filesystem instance

#### Returns

[`MockFilesystem`](interfaces/MockFilesystem.md)

***

### createMockUrlConfig()

```ts
function createMockUrlConfig(): MockUrlConfig;
```

Create a new URL config instance

#### Returns

[`MockUrlConfig`](interfaces/MockUrlConfig.md)

***

### setupMockTauri()

```ts
function setupMockTauri(options?): MockTauriContext;
```

Set up mock Tauri IPC for testing

This sets up `window.__TAURI__.core.invoke` to intercept all IPC calls
and route them to in-memory implementations. It also sets up
`__MOSS_INTERNAL_CONTEXT__` for the context-aware APIs.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `options?` | [`SetupMockTauriOptions`](interfaces/SetupMockTauriOptions.md) | Optional configuration for project path and plugin name |

#### Returns

[`MockTauriContext`](interfaces/MockTauriContext.md)

Context with mock utilities and cleanup function

#### Example

```typescript
const ctx = setupMockTauri({ projectPath: "/my/project", pluginName: "my-plugin" });

// Set up test data
ctx.filesystem.setFile("/my/project/article.md", "# Test");
ctx.urlConfig.setResponse("https://example.com/image.png", {
  status: 200,
  ok: true,
  contentType: "image/png",
  bytesWritten: 1024,
});

// Run your plugin code...

// Verify results
expect(ctx.downloadTracker.completedDownloads).toHaveLength(1);

// Cleanup
ctx.cleanup();
```
