[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / KeyInfo

# Interface: KeyInfo

A key's public face. Never includes private material.

## Properties

### algorithm

```ts
algorithm: KeyAlgorithm;
```

***

### name

```ts
name: string;
```

The name you gave the key, within your plugin's scope.

***

### publicKey

```ts
publicKey: Uint8Array;
```

The public key bytes, in the algorithm's standard encoding.
