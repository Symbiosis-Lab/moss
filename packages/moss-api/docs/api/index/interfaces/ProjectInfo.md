[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ProjectInfo

# Interface: ProjectInfo

Wire truth: `ProjectInfo` in moss's `crates/moss-build/src/plugins/types.rs`.
`project_type` and `content_folders` were removed there (2026); this type
carried them for months after — keep the two in sync.

## Properties

### folder\_name?

```ts
optional folder_name?: string;
```

Root folder basename (e.g. "刘果"). Plugins that generate a folder home
should name it self-named (`<folder_name>.md`) with a `home: true` marker
to match moss's folder-home convention.

***

### homepage\_file?

```ts
optional homepage_file?: string;
```

***

### lang

```ts
lang: string;
```

BCP-47 language code detected from content (e.g. "en", "zh-hant"). Always sent.

***

### site\_name?

```ts
optional site_name?: string;
```

***

### total\_files

```ts
total_files: number;
```
