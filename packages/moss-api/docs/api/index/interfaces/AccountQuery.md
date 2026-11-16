[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / AccountQuery

# Interface: AccountQuery

What a channel's optional `copy_account` receives. It answers which account
the channel is signed in to, in [CopyOutcome.account](CopyOutcome.md#account); moss keeps it
on each copy, and leaves a copy sent from another account alone.

## Properties

### channel

```ts
channel: string;
```

***

### config

```ts
config: Record<string, unknown>;
```

The channel's resolved settings.
