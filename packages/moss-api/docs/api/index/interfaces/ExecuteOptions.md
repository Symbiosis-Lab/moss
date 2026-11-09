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

The binary to run, named in the manifest's `requires` as
`execute_binary:<name>`. Pass the bare name to have it looked up on the
PATH. An absolute path is accepted only where moss itself would find that
binary: a PATH directory, a standard install directory such as
`/opt/homebrew/bin`, or the path `resolve_git_path` returned.

***

### env?

```ts
optional env?: Record<string, string>;
```

Environment variables to add. Only `GIT_TERMINAL_PROMPT`,
`COPYFILE_DISABLE` and `GIT_INDEX_FILE` are accepted; any other name
refuses the call. `GIT_INDEX_FILE` must be a deploy hook's
`context.index_file`.

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

Working directory: relative to the project root, or absolute, such as a
deploy hook's `context.site_dir` (default: the project root). moss refuses
one outside the project, or inside its `.moss/` folder other than
`site_dir`.
