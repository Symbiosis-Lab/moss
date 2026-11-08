[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / SetupMockTauriOptions

# Interface: SetupMockTauriOptions

Options for setting up mock Tauri environment

## Properties

### envVars?

```ts
optional envVars?: Record<string, string>;
```

Host environment variables visible to `getPluginEnvVar` (only allow-listed names are served)

***

### pluginName?

```ts
optional pluginName?: string;
```

Plugin name for internal context (default: "test-plugin")

***

### projectPath?

```ts
optional projectPath?: string;
```

Project path for internal context (default: "/test/project")
