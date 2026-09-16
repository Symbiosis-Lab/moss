# Security Policy

## Reporting a vulnerability

Use GitHub's [private vulnerability reporting](../../security/advisories/new) on this repository — do not open public issues.

We will acknowledge within 5 business days and coordinate disclosure.

## Cross-repo vulnerabilities

For bugs affecting multiple moss packages, file the advisory here on the package you installed. Maintainers will replicate the advisory on the originating package within 24h and link them via "references":
- `moss-core` is the originating repo for core engine bugs
- `moss-api` is the originating repo for TS API bugs
- This repo (if it's a plugin) is the originating repo for plugin-specific bugs

## Supported versions

The latest minor version is supported. See [moss releases](https://github.com/Symbiosis-Lab/moss-releases) for the matched moss app release.
