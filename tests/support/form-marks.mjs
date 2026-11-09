// What the round-marks gate and the landing check both measure on the
// subscribe form and on text fields. The functions named `read*` and
// `hitMisses` run in the page (they take one serialisable argument and close
// over nothing); the rest judge what they return.

/** Focus on a text field: how it is drawn, read as blurred then focused. `drawn` is the element that paints it when that is not the field (the search panel); `ground` names the colour behind the field when it is not what an ancestor paints (a page that shows it through a fixed layer). */
export function readFocus({ field, drawn, ground }) {
  const input = document.querySelector(field);
  const host = (drawn && document.querySelector(drawn)) || input;
  const paintedGround = () => {
    if (ground) return ground;
    for (let el = input; el; el = el.parentElement) {
      const bg = getComputedStyle(el).backgroundColor;
      if (!/rgba\(.*, 0\)|transparent/.test(bg)) return bg;
    }
    return ground || 'rgb(255, 255, 255)';
  };
  const read = () => {
    const c = getComputedStyle(host);
    const shadow = c.boxShadow;
    return {
      outline: c.outlineStyle !== 'none' && parseFloat(c.outlineWidth) > 0,
      offset: parseFloat(c.outlineOffset),
      outer: shadow !== 'none' && !shadow.includes('inset'),
      inset: shadow.includes('inset'),
      edge: `${c.borderBottomColor}|${c.borderTopColor}`,
      color: c.borderBottomColor,
      ground: paintedGround(),
    };
  };
  input.blur();
  const rest = read();
  input.focus();
  return { rest, on: read() };
}

/** Exactly one treatment, on the field's own edge: no outline, no shadow outside it, the edge changed or doubled inside it. */
export function focusProblem({ rest, on }) {
  const edge = on.inset || on.edge !== rest.edge;
  if (on.outline || on.outer || !edge) return `not one treatment on the field's edge: ${JSON.stringify(on)}`;
  if (contrast(on.color, on.ground) < 3) return `focus edge ${on.color} is under 3:1 on ${on.ground}`;
  return null;
}

/** The subscribe form's field and button: heights and where their text sits. */
export function readFieldButton(form) {
  const root = document.querySelector(form);
  const input = root.querySelector('.moss-input');
  const i = input.getBoundingClientRect();
  const b = root.querySelector('.moss-btn').getBoundingClientRect();
  const l = root.querySelector('.moss-btn__label').getBoundingClientRect();
  const c = getComputedStyle(input);
  const top = parseFloat(c.borderTopWidth), bottom = parseFloat(c.borderBottomWidth);
  return { input: i.height, button: b.height, offset: Math.abs(i.top + top + (i.height - top - bottom) / 2 - (l.top + l.height / 2)) };
}

export function fieldButtonProblem(m) {
  if (Math.abs(m.input - m.button) > 1) return `field ${m.input}px, button ${m.button}px`;
  if (m.offset > 1) return `field text and button label are ${m.offset}px apart`;
  return null;
}

/** The button's box, the status icon's box, and the message's line height. */
export function readMarks(form) {
  const root = document.querySelector(form);
  const box = (el) => { const r = el.getBoundingClientRect(); return { width: r.width, height: r.height }; };
  const message = [...root.querySelectorAll('.moss-subscribe-status [data-copy]')].find((s) => !s.hidden);
  const px = message ? parseFloat(getComputedStyle(message).lineHeight) : NaN;
  return {
    button: box(root.querySelector('.moss-btn')),
    icon: box(root.querySelector('.moss-subscribe-status__icon')),
    line: message ? (Number.isNaN(px) ? message.getBoundingClientRect().height : px) : NaN,
  };
}

/** The success mark is a circle one message line across; the error icon is square and no taller than the line. */
export function markProblem(kind, m) {
  if (Number.isNaN(m.line)) return 'no message to measure against';
  if (kind === 'circle') {
    const { width, height } = m.button;
    return Math.abs(width - height) > 1 || Math.abs(width - m.line) > 1 ? `the success mark is ${width} x ${height}px, not a circle ${m.line}px across` : null;
  }
  const { width, height } = m.icon;
  return Math.abs(width - height) > 1 || height > m.line + 1 ? `the error icon is ${width} x ${height}px against a ${m.line}px line` : null;
}

/** How many of eight points `reach` px from the button's centre fall outside the button. */
export function hitMisses({ form, reach }) {
  const button = document.querySelector(form).querySelector('.moss-btn');
  const box = button.getBoundingClientRect();
  const cx = box.left + box.width / 2, cy = box.top + box.height / 2;
  const r = reach;
  return [[-r, -r], [r, -r], [-r, r], [r, r], [0, -r], [0, r], [-r, 0], [r, 0]]
    .filter(([dx, dy]) => !button.contains(document.elementFromPoint(cx + dx, cy + dy))).length;
}

function luminance(rgb) {
  const [r, g, b] = (rgb.match(/[\d.]+/g) ?? []).slice(0, 3).map((v) => {
    const c = Number(v) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
