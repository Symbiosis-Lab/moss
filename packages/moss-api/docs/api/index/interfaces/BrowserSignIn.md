[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / BrowserSignIn

# Interface: BrowserSignIn

A sign-in that finishes in the person's own browser and comes back to moss.

## Properties

### redirectUri

```ts
redirectUri: string;
```

Where the provider sends the browser back: `http://127.0.0.1:<port>/callback`. Pass it as the redirect URI. Its port differs each time; a provider matches the rest exactly (RFC 8252 §7.3).

## Methods

### open()

```ts
open(authorizeUrl): Promise<URLSearchParams>;
```

Open the provider's sign-in page in the default browser and wait for it to
send the browser back. Resolves with the query of that redirect, `error`
included; check `state` yourself. Rejects with [SignInCancelled](../classes/SignInCancelled.md)
when the person closes the sign-in panel or another sign-in starts. Any
other rejection is a failure, such as "the sign-in took longer than ten
minutes" or "the sign-in stopped listening".

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `authorizeUrl` | `string` |

#### Returns

`Promise`\<`URLSearchParams`\>
