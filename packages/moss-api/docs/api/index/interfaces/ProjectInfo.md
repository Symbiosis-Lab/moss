[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / ProjectInfo

# Interface: ProjectInfo

## Properties

### content\_folders

```ts
content_folders: string[];
```

***

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

### lang?

```ts
optional lang?: string;
```

***

### project\_type

```ts
project_type: string;
```

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
