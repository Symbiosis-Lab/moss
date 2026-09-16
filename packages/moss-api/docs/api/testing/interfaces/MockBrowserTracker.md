[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockBrowserTracker

# Interface: MockBrowserTracker

Tracks browser open/close calls for testing

## Properties

### closeCount

```ts
closeCount: number;
```

Number of times closeBrowser was called

***

### htmlContent

```ts
htmlContent: string[];
```

HTML content passed to openBrowserWithHtml

***

### isOpen

```ts
isOpen: boolean;
```

Whether browser is currently open

***

### openedUrls

```ts
openedUrls: string[];
```

URLs that were opened in action panel

***

### systemBrowserUrls

```ts
systemBrowserUrls: string[];
```

URLs that were opened in system browser

## Methods

### reset()

```ts
reset(): void;
```

Reset tracking state

#### Returns

`void`
