[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DnsRecord

# Interface: DnsRecord

A single DNS record provided by deploy plugins

## Properties

### name

```ts
name: string;
```

Record name: "@" for apex, "www", etc.

***

### record\_type

```ts
record_type: string;
```

Record type: "A", "AAAA", "CNAME", "TXT", etc.

***

### ttl?

```ts
optional ttl?: number;
```

Optional TTL in seconds

***

### value

```ts
value: string;
```

Record value: IP address or hostname
