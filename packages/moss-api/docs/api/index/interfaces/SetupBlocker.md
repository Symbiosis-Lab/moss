[@symbiosis-lab/moss-api](../../README.md) / [index](../README.md) / SetupBlocker

# Interface: SetupBlocker

One reason the contribution is not ready, and what to do about it.

`id` is the routing verb: when the user submits the form, `check_setup`
runs again with this id as `SetupContext.action`. Ids beginning `moss:`
are reserved for moss's own blockers (a failed manifest `need`) and are
never dispatched to your hook.

## Properties

### field\_errors?

```ts
optional field_errors?: Record<string, string>;
```

Per-field errors keyed by field `key`. Returning the SAME blocker again
with these marks the verdict a re-ask: moss re-shows the form with the
errors and does NOT persist the submitted values.

***

### form?

```ts
optional form?: SetupForm;
```

Present when there is something to submit

***

### id

```ts
id: string;
```

***

### message?

```ts
optional message?: string;
```

What the person reads. Omit only when a form says it all.
