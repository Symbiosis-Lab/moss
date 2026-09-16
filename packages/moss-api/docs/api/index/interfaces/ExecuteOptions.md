[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ExecuteOptions

# Interface: ExecuteOptions

Options for executing a binary

## Properties

### args

```ts
args: string[];
```

Arguments to pass to the binary

***

### binaryPath

```ts
binaryPath: string;
```

Path to the binary (can be just the name if in PATH)

***

### env?

```ts
optional env?: Record<string, string>;
```

Additional environment variables

***

### onStderr?

```ts
optional onStderr?: (line) => void;
```

Callback for real-time stderr output. When provided, stderr lines are
streamed from the Rust backend via Tauri events as they are produced.
Useful for long-running processes like `git push` where you want to
show progress to the user.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `line` | `string` |

#### Returns

`void`

***

### stdin?

```ts
optional stdin?: string;
```

Data to pass to stdin (useful for commands like `git credential fill`)

***

### timeoutMs?

```ts
optional timeoutMs?: number;
```

Timeout in milliseconds (default: 60000)

***

### workingDir?

```ts
optional workingDir?: string;
```

Working directory relative to project root (default: project root itself)
