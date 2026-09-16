[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / TaskHandle

# Interface: TaskHandle

Lifecycle handle returned by `startTask()`. Calls are fire-and-await:
each method returns a promise that resolves once the Rust side has
applied the transition to the PanelTask registry. Terminal calls
(`succeeded`, `failed`, `cancelled`) remove the task from the registry's
tracking store; calling any further method on the same handle will
reject with "unknown plugin task id".

The state machine matches ADR-015 § Layer 2:

  Running ↔ Awaiting → Succeeded | Failed | Cancelled

`progress()` after `awaiting()` implicitly transitions back to Running
(no explicit `resumed()`).

## Properties

### id

```ts
readonly id: string;
```

In-process task id minted by the Rust registry on `Started`.
Exposed for log correlation and tests.

Rust-side this is `u64`; specta types `u64` as `string` because
JS numbers lose precision above 2^53. The handle carries the
exact string through subsequent transitions so no precision is
lost in the round-trip.

## Methods

### advise()

```ts
advise(advisory): Promise<void>;
```

PROPOSE an advisory on this task (Step 3 Phase 5, §8 + R13). Accumulates
the proposal on the handle; it is flushed into the next terminal call
(`succeeded`/`failed`) as `advisories: PluginAdvisory[]`. moss holds the
severity gavel server-side: a `Blocking` proposal with no actionable
`action` is clamped to a quiet `NeedsAction` dot; an actionable `Blocking`
on a `succeeded()` flips the run to `Failed` (invariant #1 — the smart
constructor decides, not the plugin).

`advise()` does NOT emit on its own; advisories ride the terminal IPC so
moss applies them atomically with the success/failure transition. Calling
`advise()` after a terminal call has no effect — the handle is spent (the
terminal methods set a spent flag and `advise()` no-ops once spent).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `advisory` | [`AdvisoryProposal`](AdvisoryProposal.md) |

#### Returns

`Promise`\<`void`\>

***

### awaiting()

```ts
awaiting(
   directive, 
   venue, 
escape?): Promise<void>;
```

Pause for an out-of-band user action. `directive` describes what
the user needs to do ("click the link in your email"); `venue`
names where ("your email") — both feed the Awaiting renderer's
"Waiting for you to [directive] in [venue]" copy.

`escape` defaults to "cancel". For non-cancel escapes, pass
`"resend:<label>"` or `"recheck:<label>"`.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `directive` | `string` |
| `venue` | `string` |
| `escape?` | [`EscapeSpec`](../README.md#escapespec) |

#### Returns

`Promise`\<`void`\>

***

### cancelled()

```ts
cancelled(): Promise<void>;
```

Terminal: explicit user cancellation.

#### Returns

`Promise`\<`void`\>

***

### failed()

```ts
failed(error, recoverable?): Promise<void>;
```

Terminal: failure. `recoverable=false` (default) also fires the
toast subscriber (ADR-015 § "Plugin-originated failure toasts").

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `error` | `string` |
| `recoverable?` | `boolean` |

#### Returns

`Promise`\<`void`\>

***

### progress()

```ts
progress(fraction?, message?): Promise<void>;
```

Push a progress update. `fraction` in [0,1] if known, else undefined for indeterminate.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `fraction?` | `number` |
| `message?` | `string` |

#### Returns

`Promise`\<`void`\>

***

### succeeded()

```ts
succeeded(receipt?, amount?): Promise<void>;
```

Terminal: success.

#### Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `receipt?` | `string` | Optional human-readable receipt (the legacy free-text path). IGNORED for the verb/amount when the task declared a `job` descriptor — moss renders the receipt from its OWN normalized verb + amount instead. |
| `amount?` | `number` | Optional success COUNT. Only meaningful when `startTask` was given a `job` id: moss pairs this count with the descriptor's `noun` to stamp `Amount { count, noun }` and renders "Syndicated · N posts" from its value objects. |

#### Returns

`Promise`\<`void`\>
