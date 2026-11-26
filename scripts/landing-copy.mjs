// The served landing pages are checked as a crawler without JavaScript reads them.
const entities = { '&amp;': '&', '&quot;': '"', '&#39;': "'", '&lt;': '<', '&gt;': '>' };

/** The words in `html` a reader gets with scripts off: markup, script, style and noscript removed. */
export function visibleText(html) {
  // Repeated until stable: one pass over `<!<!---->--` leaves a comment behind.
  let text = html, before;
  do { before = text; text = text.replace(/<!--[\s\S]*?-->/g, ''); } while (text !== before);
  return text
    .replace(/<(script|style|noscript|svg|template)\b[\s\S]*?<\/\1\s*>/gi, ' ')
    .replace(/<[^>]*>/g, ' ')
    .replace(/&(?:amp|quot|#39|lt|gt);/g, (entity) => entities[entity])
    .replace(/\s+/g, ' ')
    .trim();
}

export const LANDING_SECTIONS = ['intro', 'c1', 'c2', 'c3', 'c4', 'five', 'beta'];

/** The markup of the element with this id, up to its own closing tag. */
function elementById(html, id) {
  const open = new RegExp(`<([a-z][a-z0-9]*)\\b[^>]*\\bid="${id}"[^>]*>`, 'i').exec(html);
  if (!open) return null;
  const tag = open[1].toLowerCase();
  const tags = new RegExp(`<(/?)${tag}\\b[^>]*>`, 'gi');
  tags.lastIndex = open.index + open[0].length;
  for (let depth = 1, match; (match = tags.exec(html));) {
    depth += match[1] ? -1 : 1;
    if (!depth) return html.slice(open.index, tags.lastIndex);
  }
  return null;
}

/**
 * What is wrong with a landing page's sections with no script run: a section that is absent,
 * has no heading or has no sentence under it. Other than `en`, a heading must be in Chinese.
 */
export function sectionProblems(html, locale) {
  return LANDING_SECTIONS.flatMap((id) => {
    const section = elementById(html, id);
    if (!section) return [`#${id}: not in the HTML`];
    const heading = /<h[1-3]\b[^>]*>([\s\S]*?)<\/h[1-3]>/i.exec(section);
    const title = heading ? visibleText(heading[1]) : '';
    const rest = visibleText(section.replace(heading?.[0] ?? '', ''));
    if (!title) return [`#${id}: no heading text`];
    if (rest.length < 12) return [`#${id}: no text under its heading`];
    if (locale !== 'en' && !/\p{Script=Han}/u.test(title)) return [`#${id}: heading is not in ${locale}`];
    return [];
  });
}
