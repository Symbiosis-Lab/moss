[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / DnsTarget

# Interface: DnsTarget

DNS configuration provided by deploy plugins

Plugins are responsible for generating the appropriate DNS records
for their platform. moss just passes these through to DNS configuration.

## Properties

### records

```ts
records: DnsRecord[];
```

List of DNS records to configure
