/**
 * `as_of: today`: the stamp, and each panel's copy of it, shows the visitor's
 * own day in the stamp's language. The build wrote the page's date for a
 * reader without script.
 */
export function stampToday(figure: HTMLElement, now = new Date()): void {
  const stamp = figure.querySelector<HTMLElement>(".moss-country-stamp[data-today]");
  const time = stamp?.querySelector("time");
  if (!stamp || !time) return;
  let day: string;
  try {
    day = new Intl.DateTimeFormat(stamp.closest("[lang]")?.getAttribute("lang") || undefined, { dateStyle: "long" }).format(now);
  } catch {
    return; // A malformed language tag: the built date stays.
  }
  const text = (stamp.dataset.today || "").split("{date}").join(day);
  const pad = (n: number): string => String(n).padStart(2, "0");
  time.dateTime = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  time.textContent = text;
  figure.querySelectorAll("template").forEach((t) => t.content.querySelectorAll(".moss-country-stamp").forEach((p) => (p.textContent = text)));
}
