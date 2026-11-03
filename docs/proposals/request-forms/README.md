# Request forms: a form written in markdown, sent to a form service

**Status:** In progress, on `feat/request-forms`.

A site that takes requests (a consultation, a quote, a seat on a course) needs a form whose answers reach a person, without an email link that opens the reader's mail app and without a third-party embed in another site's look. The form is written in the page where its words are, as a list, the way `:::subscribe` is written. moss renders a plain HTML form that works with no script, and posts it to a form service named by one setting, so the service can be swapped without touching a page.

## The author's side

```markdown
:::form {#booking next=/booking/received/ button="Ask for a date"}
- Your name * {autocomplete=name}
- Email * {type=email}
- Phone {type=tel}
- Session * {type=radio}
  - Portrait
  - Family
  - Headshots
- Preferred day {type=date}
- Anything we should know {type=textarea}
  Who is coming, and any ideas for the session.
- Send me the studio's newsletter {type=checkbox}
:::
```

- `#booking` names the form (lowercase letters, digits and hyphens, at most 40). It must be unique in the site.
- Each list item is one field; its text is the field's visible label. The fence body is read as markdown (items, nested lists, continuation paragraphs) and interpreted as the form's definition; the `{…}` at the end of an item is read by the form, not by markdown.
- A trailing ` *` makes the field required: the star is not shown, and the field gets `required`. Optional fields say "Optional" in text beside their label, so the reader can tell which they may skip (the convention of public-service form design: mark the exception, and most fields in a short form are required).
- `{type=…}`: `text` (the default), `email`, `tel`, `textarea`, `select`, `radio`, `checkbox`, `date`. A nested list gives the choices of `select`, `radio` and `checkbox`; `select` and `radio` need at least two. A `checkbox` with no nested list is one yes-or-no box, sent as `on` when ticked. A paragraph continuing the item is the field's help text, tied to it with `aria-describedby`.
- Browsers fill what they know: `email` fields carry `autocomplete="email"`, `tel` fields `autocomplete="tel"`, and `{autocomplete=…}` passes any other token through (`name`, `organization`, `address-level2`).
- A field's name is its label in lowercase with words joined by `_` (`Your name` is `your_name`); `{name=…}` sets it, and a label with no Latin letters (a Chinese label, say) must set it. Names match `[a-z][a-z0-9_]{0,39}`; `_next` and `_hp` are reserved.
- `next=` is required: the page the reader lands on after sending, a path in the site. `button=` is the submit label (localised "Send" by default).
- A visible field can be filled from the page's address: `/booking/?session=Family` selects that choice. Hidden fields are never filled this way, and only the value is set, so a link cannot mark a request as spam or change where it lands.

## Where it is sent

`[services.forms] endpoint = "<url>"` in `.moss/config.toml`. A form posts to `<endpoint>/<form>`. A site published to moss hosting will get moss's own form service as the default once that service exists. Until a form has an address to post to, the build warns, naming the setting; the form stays in the markup but is hidden on published pages (the rule subscribe uses for a form with no site yet), so a reader never meets a form that cannot send; and moss's own preview shows it, with sending simulated.

Every form also carries two hidden fields the service reads: `_next`, the `next=` path, and `_hp`, an empty honeypot that people never see or reach by keyboard and that the service treats as spam when filled.

## What the build writes

Besides the form markup, the build writes the site's public commerce description, `/_moss/commerce.json`, at a fixed address so a service can fetch it by the site's URL: `{"forms": {"request": {"fields": [{"name": "your_name", "type": "text", "required": true}, …], "next": "/request/received/", "turnstile": false}}}`. `turnstile` tells the service whether to hold a post it cannot check for review; it is always `false` in this change, because moss renders no widget, so the service accepts a post normally, guarded by the honeypot and its own rate limit. Labels and help text stay in the page; who receives a form is set in the service, never in the folder. The file is written whenever the site has a form or a `[services.forms]` setting, so removing the last form leaves `{"forms": {}}` at the address rather than a missing file.

## What the reader sees

- **No script:** the browser checks `required` and each `type` before sending; the post goes to the service, which answers with a redirect to `next` or with its own page naming a refused field.
- **With script:** the form posts the same `application/x-www-form-urlencoded` body with `Accept: application/json` and stays on the page while sending. Accepted: the reader goes to `next`. A field refused (`422` with `fields`): each named field is marked invalid with its message, and focus moves to the first; everything typed stays. Too many sends (`429`), an unknown form (`404`) or no connection: one line under the button says so, with the answers kept.
- **moss's preview:** sending is simulated, so an author sees the flow to the `next` page.

## Build problems (a warning, a failure under `--strict`)

Two forms with one name; an invalid form or field name; two fields with one name in a form; an unknown `type`; a `select` or `radio` with fewer than two choices, or choices on a type that takes none; a label with no Latin letters and no `{name=…}`; a reserved name; `next=` that is missing, is not a path, or names no page the build wrote.

## Not in this change

Cloudflare Turnstile (no widget is rendered, and each form's `turnstile` key is written as `false`); the call that tells the service a new description was published (it waits for the service); file uploads; multi-step forms.
