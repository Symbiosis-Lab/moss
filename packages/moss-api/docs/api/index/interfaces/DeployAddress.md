[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DeployAddress

# Interface: DeployAddress

One way to reach the published site.

A publish usually produces several: the CID that names these exact bytes,
the IPNS name that will name the next ones too, the gateway URL that makes
either reachable from a browser.

Give an address a `url` when it opens in a browser, a `value` when it is
also (or only) worth copying — moss shows both when both are given, a
copy button when only `value` is set, and an open link when only `url` is.

## Properties

### kind

```ts
kind: AddressKind | string & object;
```

***

### label

```ts
label: string;
```

Shown verbatim as the row's label, e.g. "IPFS CID".

***

### note?

```ts
optional note?: string;
```

One line of context shown beside it, e.g. "Public gateway, may be slow".

***

### url?

```ts
optional url?: string;
```

Openable in a browser.

***

### value?

```ts
optional value?: string;
```

The literal string to copy — beside `url`, or on its own.
