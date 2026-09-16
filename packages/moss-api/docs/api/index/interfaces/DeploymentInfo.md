[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DeploymentInfo

# Interface: DeploymentInfo

Deployment result information

## Properties

### addresses?

```ts
optional addresses?: DeployAddress[];
```

Every way to reach what was just published. moss keeps these in the
deployment record and lists them in the deploy tab whenever it is open,
so an address returned here outlives the toast that announced it.

***

### deployed\_at

```ts
deployed_at: string;
```

***

### dns\_target?

```ts
optional dns_target?: DnsTarget;
```

DNS target for custom domain configuration

***

### metadata

```ts
metadata: Record<string, string>;
```

***

### method

```ts
method: string;
```

***

### url

```ts
url: string;
```
