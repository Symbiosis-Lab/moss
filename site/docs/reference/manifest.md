---
title: Manifest
uid: 8ec7c709
weight: 20
description: Plugin manifest reference, configuration, and the moss-api SDK.
translationKey: docs-extend-manifest
---

## Directory structure

```
.moss/plugins/my-plugin/
├── manifest.json      Plugin metadata and config
├── main.bundle.js     Built JavaScript entry point
├── config.json        Runtime config (overrides defaults)
└── icon.svg           Plugin icon (optional)
```

Each plugin lives in `.moss/plugins/<name>/` inside the project.

## Manifest fields

<!-- auto:start:manifest -->
| Field | Type | Required | Description |
|---|---|---|---|
| `name` | `string` | yes | Plugin identifier (e.g. "matters"). Must match the plugin directory name. |
| `version` | `string` | yes | Plugin version in semver format (e.g. "1.0.0"). |
| `entry` | `string` | yes | Entry point JavaScript file path, relative to the plugin directory. |
| `capabilities` | `string[]` | no | Hook names this plugin implements (e.g. ["process", "syndicate"]). Each must be a valid hook name. An empty or omitted capabilities field is valid (plugin implements no hooks). |
| `description` | `string` | no | Human-readable description of what the plugin does. |
| `author` | `string` | no | Plugin author name or contact. |
| `global_name` | `string` | no | Global JavaScript variable name the plugin exports. Defaults to PascalCase(name) + "Plugin". |
| `display_name` | `string` | no | Human-readable name shown in the moss Settings UI section title (e.g. "Comments"). |
| `icon` | `string` | no | Path to the plugin icon file, relative to the plugin directory. Falls back to icon.svg / icon.png / logo.svg / logo.png. |
| `domain` | `string` | no | Primary domain for cookie access (e.g. "matters.town"). Required for plugins using cookie-based authentication. |
| `domains` | `string[]` | no | Full set of domains the plugin operates on (e.g. prod + staging). Used for scope-wide cookie clearing on force-fresh login. |
| `config` | `object` | no | Plugin-specific configuration key-value pairs (e.g. login_url, api_endpoint). Merged with .moss/config.toml at runtime. |
| `config_schema` | `object` | no | Legacy alias: field name -> type string, folded at parse into the contribution's setup.settings[]. New manifests declare settings[] directly. |
| `config_labels` | `object` | no | Legacy alias: field name -> label, folded into settings[].label. A dotted "<field>.<value>" key labels one enum option. |
| `config_descriptions` | `object` | no | Legacy alias: field name -> help text, folded into settings[].description. |
| `config_options` | `object` | no | Legacy alias: field name -> allowed values, folded into settings[].options on a string field. |
| `config_placeholders` | `object` | no | Legacy alias: field name -> placeholder text, folded into settings[].placeholder. |
| `contributes` | `object` | no | Contributions (deploy targets, frontmatter fields, embed renderers, job descriptors, stack declarations), VS Code style. Each contribution may carry a setup block — needs (host-checked preconditions like "stack"), settings (fields moss draws, the Field vocabulary), credentials, and check (the plugin implements the check_setup hook). A stack contribution carries its own artifact pin (id, version, sources) instead of a setup block. |
| `min_moss_version` | `string` | no | Minimum moss version this plugin supports (semver). The registry client checks it at install and load; older moss ignores the field. |
| `repository` | `string` | no | Source repository URL. Display and provenance only. |
| `homepage` | `string` | no | Homepage URL. Display only. |
| `requires_stack` | `boolean` | no | Legacy alias: parses as needs: ["stack"] on every contribution. New manifests declare the need inside the contribution's setup block. |
| `requires` | `array` | no | Declared host-capability requirements, named per binary: "execute_binary:<basename>" grants exactly one executable (e.g. "execute_binary:git"). The bare "execute_binary" blanket is a deprecated alias that still grants every binary, with a warning per run. |
<!-- auto:end:manifest -->

## Example manifest

```json
{
  "name": "my-deploy",
  "version": "1.0.0",
  "description": "Deploy to My Hosting",
  "author": "Your Name",
  "entry": "main.bundle.js",
  "capabilities": ["deploy"],
  "global_name": "MyDeployPlugin",
  "domain": "myhost.example",
  "icon": "icon.svg",
  "config": {
    "auto_deploy": false,
    "region": "us-east"
  },
  "config_schema": {
    "auto_deploy": "boolean",
    "region": "string"
  },
  "config_labels": {
    "auto_deploy": "Auto Deploy",
    "region": "Server Region"
  },
  "config_descriptions": {
    "auto_deploy": "Automatically deploy after each build",
    "region": "Hosting region for your site"
  }
}
```

## Configuration

### Defaults and schema

`config` sets default values. `config_schema` declares the type of each field (`"boolean"`, `"number"`, `"string"`). moss generates a settings UI from these fields automatically.

| Manifest field | Purpose |
|---------------|---------|
| `config` | Default values |
| `config_schema` | Field types for UI generation |
| `config_labels` | Display labels in settings |
| `config_descriptions` | Help text in settings |
| `config_placeholders` | Placeholder text for inputs |

### Config resolution

Plugin configuration is resolved in priority order:

1. `.moss/plugins/<name>/config.json` (highest priority)
2. `.moss/plugins/<name>/config.toml`
3. `.moss/config.toml` `[plugins.<name>]` section (lowest)

### Config verification

`config_verify` probes an endpoint after the user saves config:

```json
"config_verify": {
  "api_key": {
    "probe": "https://api.example.com/verify/{value}",
    "expect": "ok"
  }
}
```

The `value` placeholder in the probe URL is replaced with the user's input. moss probes the URL and checks the response.

## Schema contributions

Plugins can add frontmatter fields via `contributes.frontmatter.fields`:

```json
"contributes": {
  "frontmatter": {
    "fields": {
      "syndicated_url": {
        "type": "string",
        "description": "URL where this article was syndicated"
      }
    }
  }
}
```

Contributed fields are merged into the active schema at runtime and appear in the moss editor.

## moss-api SDK

The `@symbiosis-lab/moss-api` package provides types and utilities for plugin development:

```sh
npm install @symbiosis-lab/moss-api
```

### Types

| Export | Description |
|--------|-------------|
| `DeployContext` | Passed to deploy hooks |
| `SyndicateContext` | Passed to syndicate hooks |
| `EnhanceContext` | Passed to enhance hooks |
| `HookResult` | Return type for all hooks |
| `PluginManifest` | Manifest shape |

### Utilities

| Export | Description |
|--------|-------------|
| `reportProgress` | Show progress message in the UI |
| `reportError` | Report a non-fatal error |
| `log` / `warn` / `error` | Structured logging |

### Browser

| Export | Description |
|--------|-------------|
| `openBrowser` | Open a URL in the system browser |
| `closeBrowser` | Close a previously opened browser tab |

## Building

Bundle with esbuild as an IIFE:

```sh
esbuild src/main.ts --bundle --format=iife --global-name=MyPlugin --outfile=dist/main.bundle.js
```

## Testing

Use vitest with the mock Tauri setup from `moss-api`:

```typescript
import { describe, it, expect } from 'vitest';

describe('deploy', () => {
  it('returns success', async () => {
    const result = await MyPlugin.deploy(mockContext);
    expect(result.success).toBe(true);
  });
});
```

```sh
npx vitest run
```
