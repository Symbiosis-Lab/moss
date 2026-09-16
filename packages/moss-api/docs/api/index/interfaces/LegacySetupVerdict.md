[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / LegacySetupVerdict

# ~~Interface: LegacySetupVerdict~~

## Deprecated

The pre-contract verdict shape. moss still accepts it — each
need's action becomes a blocker whose zero-field form carries the action's
label, and `consent` folds into the message — but new code answers with
[SetupVerdict](../README.md#setupverdict).

## Properties

### ~~needs?~~

```ts
optional needs?: object[];
```

#### ~~actions?~~

```ts
optional actions?: object[];
```

#### ~~id~~

```ts
id: string;
```

#### ~~message~~

```ts
message: string;
```

***

### ~~ready~~

```ts
ready: boolean;
```
