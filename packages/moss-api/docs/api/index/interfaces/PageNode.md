[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / PageNode

# Interface: PageNode

A node in the universal Page Tree.

Every markdown file and every folder produces one PageNode.
Folders have is_folder=true and may have children.
This is the universal intermediate representation consumed by
both the built-in generator and SSG plugins.

## Properties

### also\_in

```ts
also_in: string[];
```

Folder paths where this article also appears in child lists

***

### children

```ts
children: PageNode[];
```

Child nodes (populated for folders)

***

### content\_html

```ts
content_html: string;
```

Rendered HTML content (empty string for auto-generated folder pages)

***

### cover?

```ts
optional cover?: string;
```

Cover image path

***

### date?

```ts
optional date?: string;
```

Publication date (ISO string)

***

### draft

```ts
draft: boolean;
```

Whether this page is a draft — rendered and published at its direct URL but hidden from listings, feeds, and navigation.

***

### flatten

```ts
flatten: boolean;
```

Recursively list all nested content

***

### frontmatter

```ts
frontmatter: Record<string, unknown>;
```

Raw frontmatter for plugin-specific fields

***

### is\_folder

```ts
is_folder: boolean;
```

Whether this node represents a folder

***

### list\_style

```ts
list_style: "list" | "grid" | "sidebar";
```

How children display: "list", "grid", or "sidebar"

***

### nav

```ts
nav: boolean;
```

Whether this page appears in header navigation

***

### nav\_weight?

```ts
optional nav_weight?: number;
```

Navigation ordering (lower = first)

***

### slug

```ts
slug: string;
```

URL-safe slug

***

### source\_path

```ts
source_path: string;
```

Relative source path (e.g., "articles/hello.md" or "articles")

***

### title

```ts
title: string;
```

Page title (from frontmatter, H1, or filename)

***

### url\_path

```ts
url_path: string;
```

Output URL path (e.g., "articles/hello.html")
