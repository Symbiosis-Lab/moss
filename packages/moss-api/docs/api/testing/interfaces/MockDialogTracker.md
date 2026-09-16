[@symbiosis-lab/moss-api](../../README.md) / [testing](../README.md) / MockDialogTracker

# Interface: MockDialogTracker

Tracks dialog interactions for testing

## Properties

### nextResult

```ts
nextResult: MockDialogResult | null;
```

Configure the next dialog result (for automatic response)

***

### shownDialogs

```ts
shownDialogs: object[];
```

Dialogs that were shown (with their URLs and titles)

#### height

```ts
height: number;
```

#### title

```ts
title: string;
```

#### url

```ts
url: string;
```

#### width

```ts
width: number;
```

***

### submittedResults

```ts
submittedResults: Map<string, MockDialogResult>;
```

Submitted results by dialog ID

## Methods

### cancelDialog()

```ts
cancelDialog(dialogId): void;
```

Simulate user cancelling a dialog

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `dialogId` | `string` |

#### Returns

`void`

***

### reset()

```ts
reset(): void;
```

Reset tracking state

#### Returns

`void`

***

### setNextResult()

```ts
setNextResult(result): void;
```

Set the result for the next dialog shown

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `result` | [`MockDialogResult`](MockDialogResult.md) |

#### Returns

`void`

***

### submitResult()

```ts
submitResult(dialogId, value): void;
```

Simulate user submitting a dialog result

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `dialogId` | `string` |
| `value` | `unknown` |

#### Returns

`void`
