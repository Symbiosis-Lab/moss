/**
 * site-settings-form.ts — a plugin's `site_settings`, drawn from its manifest
 * and written into the plugin's own table of the site's config.
 *
 * The form holds no copy to save from: every commit sends the key paths it
 * changed, and the host writes them into the file as it is on disk, so
 * another writer's edit between two commits is kept. The table passed in is
 * updated only after a write lands, so `when` reads what the file holds.
 */

import { createFieldRow, drawFieldInput, whenMatches, type FieldInput, type SiteSettingField as SettingField, type SettingValue } from "./setting-field-input";
import { createPillInput } from "./pill-select";
import { createToggle } from "./toggle";
import { bindAction } from "./run-action";
import { t } from "../../i18n/index";

export type SettingTable = { [key: string]: SettingValue };

/** A key path below the plugin's table, and its new value or `null` to remove it. */
export type SettingChange = [string[], SettingValue | null];

/** A button the manifest declares; what it calls is the host's business. */
export interface SiteSettingAction {
  id: string;
  label: string;
  group?: string | null;
}

export interface SiteSettingsFormOptions {
  /** Write every change in one save; resolves false when it failed and nothing was written. */
  write(changes: SettingChange[]): Promise<boolean>;
  actions?: SiteSettingAction[];
  /** Run an action's call; a rejection or an error result is shown by the button. */
  run?(action: SiteSettingAction): Promise<unknown>;
}

const SAVED_MS = 2000;

/** Draw `fields` over `values` into `container`, which gets the settings row grammar. */
export function drawSiteSettings(
  container: HTMLElement,
  fields: SettingField[],
  values: SettingTable,
  options: SiteSettingsFormOptions,
): void {
  container.classList.add("moss-info-grid");
  // A choice that adds or removes keys redraws from the live table, so every
  // input shows what was just written.
  const draw = () => {
    container.replaceChildren();
    drawTable(container, fields, values, [], ctx).refresh(true);
    drawActions(container, options);
  };
  const ctx: Ctx = { write: options.write, redraw: draw };
  draw();
}

interface Ctx {
  write(changes: SettingChange[]): Promise<boolean>;
  redraw(): void;
}

/**
 * A manifest's strings in the reader's language. `translations` is keyed by
 * locale and then by the English text; `zh-hans` falls back to `zh`.
 */
export function translateFields(
  fields: SettingField[],
  translations: Record<string, Record<string, string>> | undefined,
  locale: string,
): SettingField[] {
  const tr = manifestWords(translations, locale);
  const one = (f: SettingField): SettingField => ({
    ...f,
    label: tr(f.label),
    description: tr(f.description),
    group: tr(f.group),
    placeholder: tr(f.placeholder),
    pattern_message: tr(f.pattern_message),
    unit: tr(f.unit),
    add_label: tr(f.add_label),
    options: f.options?.map((o) => ({ ...o, label: tr(o.label) ?? o.label, description: tr(o.description) })),
    fields: f.fields?.map(one),
    items: f.items ? one(f.items) : f.items,
  });
  return fields.map(one);
}

/** One manifest string in the reader's language, or as written. */
export function manifestWords(
  translations: Record<string, Record<string, string>> | undefined,
  locale: string,
): <T extends string | null | undefined>(text: T) => T {
  const table = translations?.[locale] ?? translations?.[locale.split("-")[0] ?? ""] ?? {};
  return (text) => (text ? ((table[text] ?? text) as typeof text) : text);
}

/** A value there to keep: an empty table standing in for an absent object is not. */
function filled(v: SettingValue | undefined): boolean {
  if (v === undefined || v === null) return false;
  return typeof v !== "object" || Array.isArray(v) || Object.keys(v).length > 0;
}

function asTable(v: SettingValue | undefined): SettingTable | null {
  return v && typeof v === "object" && !Array.isArray(v) ? v : null;
}

/**
 * The table as `when` reads it. A closed select with no stored value is read
 * from the fields it reveals: the one option whose fields hold values, else
 * the first. A table written before the select existed draws as what it is.
 */
function effective(fields: SettingField[], table: SettingTable): SettingTable {
  const out: SettingTable = { ...table };
  for (const select of fields) {
    const choices = select.options ?? [];
    if (choices.length === 0 || filled(table[select.key])) continue;
    const revealing = choices.filter((o) => fields.some((f) => f.when?.[select.key] === o.value && filled(table[f.key])));
    out[select.key] = (revealing.length === 1 ? revealing[0] : choices[0])!.value;
  }
  return out;
}

const shows = (field: SettingField, fields: SettingField[], table: SettingTable) => whenMatches(field, effective(fields, table));

/** A new table's starting values: each member's default, under the choices those defaults make. */
function seed(fields: SettingField[]): SettingTable {
  const out: SettingTable = {};
  for (const f of fields) {
    if (!shows(f, fields, out)) continue;
    const start = f.type === "object" && !f.optional ? seed(f.fields ?? []) : f.default;
    if (filled(start ?? undefined)) out[f.key] = start as SettingValue;
  }
  return out;
}

function heading(text: string): HTMLElement {
  const h = document.createElement("h3");
  h.className = "moss-settings-section-title";
  h.textContent = text;
  return h;
}

function feedbackSlot(key: string): HTMLElement {
  const span = document.createElement("span");
  span.className = "moss-input-feedback";
  span.setAttribute("data-feedback-for", key);
  return span;
}

/** Say how a save went in `slot`; "Saved" fades, a failure stays. */
function report(slot: HTMLElement | null, ok: boolean, message?: string): void {
  if (!slot) return;
  slot.textContent = message ?? t(ok ? "schema.saved" : "schema.failed_to_save");
  slot.className = `moss-input-feedback moss-input-feedback--${ok ? "success" : "error"}`;
  if (ok) {
    setTimeout(() => {
      slot.textContent = "";
      slot.className = "moss-input-feedback";
    }, SAVED_MS);
  }
}

const slotIn = (el: HTMLElement, key: string) => el.querySelector<HTMLElement>(`[data-feedback-for="${CSS.escape(key)}"]`);

interface Drawn {
  /** Show or hide every element this table drew, re-answering each `when`. */
  refresh(visible: boolean): void;
}

type Place = (el: HTMLElement) => void;

interface Member {
  field: SettingField;
  els: HTMLElement[];
  /** The member's own nested table, refreshed with it. */
  inner?: Drawn;
}

function drawTable(host: HTMLElement, fields: SettingField[], table: SettingTable, path: string[], ctx: Ctx): Drawn {
  const members: Member[] = [];
  let shown = true;
  const drawn: Drawn = {
    refresh(visible) {
      shown = visible;
      for (const m of members) {
        const on = visible && shows(m.field, fields, table);
        for (const el of m.els) el.hidden = !on;
        m.inner?.refresh(on && (!m.field.optional || table[m.field.key] !== undefined));
      }
    },
  };
  const refresh = () => drawn.refresh(shown);

  let previousGroup: string | null = null;
  for (const field of fields) {
    const els: HTMLElement[] = [];
    const group = field.group || null;
    // Each element goes in once, in drawing order: an object draws its rows after its own heading.
    const place: Place = (el) => {
      els.push(host.appendChild(el));
    };
    if (group && group !== previousGroup) place(heading(group));
    previousGroup = group;
    const member: Member = { field, els };
    const at = [...path, field.key];
    if (field.type === "object") drawObject(host, member, table, at, ctx, place);
    else if (field.type === "map") drawMap(host, member, table, at, ctx, place);
    else if (field.type === "array") place(drawArray(field, table, at, ctx));
    else place(drawScalar(field, fields, table, at, ctx, refresh));
    members.push(member);
  }
  return drawn;
}

/**
 * Save one scalar the person committed, with whatever else the change
 * implies (`also`), as one write; the table changes only once it landed.
 */
async function saveScalar(
  input: FieldInput,
  table: SettingTable,
  key: string,
  path: string[],
  ctx: Ctx,
  also: (next: SettingTable) => SettingChange[] = () => [],
): Promise<boolean> {
  if (!input.valid()) return false;
  const value = input.value();
  if (JSON.stringify(value) === JSON.stringify(table[key] ?? null)) return false;
  const next: SettingTable = { ...table };
  if (value === null) delete next[key];
  else next[key] = value;
  const changes: SettingChange[] = [[path, value], ...also(next)];
  const parent = path.slice(0, -1);
  const ok = await ctx.write(changes).catch(() => false);
  if (ok) {
    for (const [p, v] of changes) {
      const k = p[parent.length]!;
      if (v === null) delete table[k];
      else table[k] = v;
    }
  }
  report(slotIn(input.el, key), ok);
  return ok;
}

function drawScalar(
  field: SettingField,
  fields: SettingField[],
  table: SettingTable,
  path: string[],
  ctx: Ctx,
  refresh: () => void,
): HTMLElement {
  const key = field.key;
  const dependents = fields.filter((f) => f.when && key in f.when);
  const initial = dependents.length > 0 ? effective(fields, table)[key] : table[key];
  const input = drawFieldInput(field, initial, { onCommit: () => void save() });
  const parent = path.slice(0, -1);
  // A choice that reveals other fields removes the keys of the fields it now
  // hides and starts the ones it shows at their defaults, in the same write,
  // so the table never holds two kinds at once.
  const consequences = (next: SettingTable): SettingChange[] => {
    const removed = dependents
      .filter((d) => !shows(d, fields, next) && filled(next[d.key]))
      .map((d): SettingChange => [[...parent, d.key], null]);
    const started = dependents
      .filter((d) => shows(d, fields, next) && !filled(next[d.key]))
      .map((d): SettingChange => [[...parent, d.key], d.type === "object" ? seed(d.fields ?? []) : (d.default ?? null)])
      .filter(([, v]) => filled(v ?? undefined));
    return [...removed, ...started];
  };
  const save = async () => {
    if (!(await saveScalar(input, table, key, path, ctx, consequences))) return;
    if (dependents.length > 0) ctx.redraw();
    else refresh();
  };
  return input.el;
}

function drawObject(host: HTMLElement, member: Member, table: SettingTable, path: string[], ctx: Ctx, place: Place): void {
  const { field } = member;
  const key = field.key;
  const inner = asTable(table[key]) ?? {};
  if (field.optional) {
    const { row, body } = createFieldRow({ label: field.label ?? key, switch: true, dataset: { field: key } });
    const toggle = createToggle({
      checked: table[key] !== undefined,
      ariaLabel: field.label ?? key,
      onChange: async (on) => {
        const value = on ? seed(field.fields ?? []) : null;
        const ok = await ctx.write([[path, value]]).catch(() => false);
        report(slotIn(row, key), ok);
        if (!ok) return;
        if (value) table[key] = value;
        else delete table[key];
        ctx.redraw();
      },
    });
    body.append(toggle, feedbackSlot(key));
    place(row);
  } else {
    place(heading(field.label ?? key));
    // Members of an absent table draw empty; the first write creates it.
    if (table[key] === undefined) table[key] = inner;
  }
  member.inner = drawTable(host, field.fields ?? [], inner, path, ctx);
}

function drawMap(host: HTMLElement, member: Member, table: SettingTable, path: string[], ctx: Ctx, place: Place): void {
  const { field, els } = member;
  const items = field.items;
  if (!items) return;
  place(heading(field.label ?? field.key));
  const entries: SettingTable = asTable(table[field.key]) ?? (table[field.key] = {});

  const add = document.createElement("div");
  add.className = "moss-site-settings-add";
  const idInput = createPillInput({ type: "text", value: "", placeholder: field.placeholder ?? undefined });
  idInput.setAttribute("aria-label", field.add_label ?? t("site_settings.add"));
  const addButton = document.createElement("button");
  addButton.type = "button";
  addButton.className = "moss-link";
  addButton.textContent = field.add_label ?? t("site_settings.add");
  const addSlot = feedbackSlot(`${field.key}:add`);
  add.append(idInput, addButton, addSlot);

  const drawEntry = (id: string) => {
    const el = drawMapEntry(field, items, entries, id, [...path, id], ctx, () => {
      els.splice(els.indexOf(el), 1);
      el.remove();
    });
    host.insertBefore(el, add);
    els.push(el);
  };

  // The manifest reader gives every map its `key_pattern`.
  const pattern = new RegExp(field.key_pattern ?? "^$");
  const tryAdd = async () => {
    const id = idInput.value.trim();
    const problem = !pattern.test(id) ? t("site_settings.id_invalid") : id in entries ? t("site_settings.id_taken", { id }) : null;
    if (problem) return report(addSlot, false, problem);
    const start: SettingValue = items.type === "object" ? seed(items.fields ?? []) : (items.default ?? items.options?.[0]?.value ?? "");
    if (!(await ctx.write([[[...path, id], start]]).catch(() => false))) return report(addSlot, false);
    addSlot.textContent = "";
    entries[id] = start;
    idInput.value = "";
    drawEntry(id);
  };
  addButton.addEventListener("click", () => void tryAdd());
  idInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void tryAdd();
    }
  });

  place(add);
  for (const id of Object.keys(entries)) drawEntry(id);
}

function drawMapEntry(
  map: SettingField,
  items: SettingField,
  entries: SettingTable,
  id: string,
  path: string[],
  ctx: Ctx,
  removed: () => void,
): HTMLElement {
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "moss-link";
  remove.textContent = t("site_settings.remove");
  remove.addEventListener("click", async () => {
    if (!(await ctx.write([[path, null]]).catch(() => false))) return;
    delete entries[id];
    removed();
  });

  if (items.type !== "object") {
    const input = drawFieldInput({ ...items, key: id, label: id }, entries[id], {
      // A blank entry keeps its value: removing one is the Remove button.
      onCommit: () => void (input.value() !== null && saveScalar(input, entries, id, path, ctx)),
    });
    input.el.dataset.entry = `${map.key}:${id}`;
    input.el.querySelector(".moss-field-body")?.appendChild(remove);
    return input.el;
  }

  const card = document.createElement("div");
  card.className = "moss-site-settings-entry";
  card.dataset.entry = `${map.key}:${id}`;
  const head = document.createElement("div");
  head.className = "moss-site-settings-entry__head";
  const title = document.createElement("span");
  title.className = "moss-site-settings-entry__id";
  title.textContent = id;
  head.append(title, remove);
  const grid = document.createElement("div");
  grid.className = "moss-info-grid";
  card.append(head, grid);
  const entry = asTable(entries[id]) ?? (entries[id] = {});
  drawTable(grid, items.fields ?? [], entry, path, ctx).refresh(true);
  return card;
}

function drawArray(field: SettingField, table: SettingTable, path: string[], ctx: Ctx): HTMLElement {
  const key = field.key;
  const { row, body } = createFieldRow({ label: field.label ?? key, dataset: { field: key } });
  const current = Array.isArray(table[key]) ? (table[key] as SettingValue[]) : [];
  const input = createPillInput({ type: "text", value: current.join(", "), placeholder: field.placeholder ?? undefined });
  input.setAttribute("aria-label", field.label ?? key);
  // Wide enough for every value it holds; the grid caps it at the column.
  input.size = Math.max(20, input.value.length + 2);
  const slot = feedbackSlot(key);
  body.append(input, slot);
  const item = field.items;
  const pattern = item?.pattern ? new RegExp(item.pattern) : null;

  const save = async () => {
    const parts = input.value.split(",").map((p) => p.trim()).filter((p) => p !== "");
    if (pattern && parts.some((p) => !pattern.test(p))) {
      input.setAttribute("aria-invalid", "true");
      return report(slot, false, item?.pattern_message ?? undefined);
    }
    input.removeAttribute("aria-invalid");
    const next: SettingValue[] = item?.type === "number" ? parts.map(Number) : parts;
    if (JSON.stringify(next) === JSON.stringify(table[key] ?? [])) return;
    const ok = await ctx.write([[path, next]]).catch(() => false);
    if (ok) table[key] = next;
    report(slot, ok);
  };
  input.addEventListener("blur", () => void save());
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      input.blur();
    }
  });
  return row;
}

/** The manifest's buttons, under their group headings, after the fields. */
function drawActions(host: HTMLElement, options: SiteSettingsFormOptions): void {
  const run = options.run;
  if (!run) return;
  let previousGroup: string | null = null;
  for (const action of options.actions ?? []) {
    const group = action.group || null;
    if (group && group !== previousGroup) host.appendChild(heading(group));
    previousGroup = group;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-link";
    button.dataset.action = action.id;
    button.textContent = action.label;
    bindAction(button, { run: () => run(action) });
    const row = document.createElement("div");
    row.className = "moss-site-settings-actions";
    row.appendChild(button);
    host.appendChild(row);
  }
}
