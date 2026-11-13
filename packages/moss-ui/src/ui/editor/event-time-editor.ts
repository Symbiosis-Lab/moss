// The editor for a `format: "event-time"` field: a date, and a time that may
// stay empty. The build reads `YYYY-MM-DD` (all day) or `YYYY-MM-DD HH:MM`,
// so a date alone is a complete value and the time is never invented.

import { t } from "../i18n/index";

interface EventTimeParts { date: string; time: string; sep: string }

const EVENT_TIME = /^(\d{4}-\d{2}-\d{2})(?:([ T])(\d{2}:\d{2}))?$/;

/** The parts of a value the build accepts; all empty for anything else. */
function parts(value: unknown): EventTimeParts {
  const m = EVENT_TIME.exec(String(value ?? '').trim());
  return { date: m?.[1] ?? '', time: m?.[3] ?? '', sep: m?.[2] ?? ' ' };
}

export interface EventTimeEditorOptions {
  /** The value in the file. */
  stored: unknown;
  /** A date to show when the file has none; written only once accepted. */
  suggestion?: unknown;
  /** The field's name, read out for the date. */
  label: string;
  /** A new value, or `null` when the person cleared the date. */
  commit(value: string | null): void;
  close(): void;
}

export function buildEventTimeEditor(o: EventTimeEditorOptions): HTMLElement {
  const stored = parts(o.stored);
  const shown = stored.date ? stored : parts(o.suggestion);

  const row = document.createElement('div');
  row.className = 'pop-event-time';
  const input = (type: 'date' | 'time', value: string, label: string) => {
    const el = document.createElement('input');
    el.type = type;
    el.className = 'pop-input';
    el.value = value;
    el.setAttribute('aria-label', label);
    row.appendChild(el);
    return el;
  };
  const date = input('date', shown.date, o.label);
  const time = input('time', shown.time, t('chip.ui.time'));

  // What the file holds, in the controls' terms. Writing only when the
  // controls differ from it keeps an untouched value byte-identical, whatever
  // separator or malformed text it carries.
  let written = { date: stored.date, time: stored.time };
  const write = () => {
    // A half-typed date or time reads as empty; it is not a cleared one, so it
    // counts as unchanged. The browser reports seconds when a step asks for
    // them; the build reads none.
    const now = {
      date: date.validity.badInput ? written.date : date.value,
      time: time.validity.badInput ? written.time : time.value.slice(0, 5),
    };
    if (now.date === written.date && now.time === written.time) return;
    // A time alone is not a value, and must not remove what the file has.
    if (!now.date && !written.date) return;
    written = now;
    o.commit(now.date ? now.date + (now.time ? stored.sep + now.time : '') : null);
  };

  for (const el of [date, time]) {
    el.addEventListener('change', write);
    el.addEventListener('keydown', (e) => {
      if (e.key !== 'Enter' && e.key !== 'Escape') return;
      // Closing removes the focused input; unconsumed, WebKit finishes the
      // keystroke in whatever takes focus next, the page body included.
      e.preventDefault();
      if (e.key === 'Enter') write();
      o.close();
    });
  }
  return row;
}
