import { beforeEach, describe, expect, test, vi } from 'vitest';
import { ChipDiagnostics, type ValidateFn } from '../chip-diagnostics';

const validate = vi.fn() as unknown as ReturnType<typeof vi.fn> & ValidateFn;

const diag = (path: string | null, message: string, severity = 2) =>
  ({ severity, message, path, line: null, column: null });

function chipBar(...keys: string[]): HTMLElement {
  const bar = document.createElement('div');
  bar.innerHTML = keys.map((k) => `<span class="chip" data-key="${k}"></span>`).join('');
  return bar;
}

describe('ChipDiagnostics', () => {
  let d: ChipDiagnostics;
  beforeEach(() => {
    d = new ChipDiagnostics(validate);
    validate.mockReset();
  });

  test('marks only the chip the diagnostic names', async () => {
    validate.mockResolvedValue({
      status: 'ok',
      data: [diag('url', 'Another page already uses this address: 獎項/漫畫獎/漫畫獎.md')],
    } as never);

    const bar = chipBar('title', 'url', 'date');
    await d.load('/vault/獎項/記憶獎/記憶獎.md');
    d.decorate(bar);

    const url = bar.querySelector<HTMLElement>('[data-key="url"]')!;
    expect(url.classList.contains('chip--diagnostic')).toBe(true);
    expect(url.dataset.tooltip).toContain('漫畫獎');
    // The message has to reach a screen reader; a chip is a click target, not
    // a heading, so its text content is not where the sentence lives.
    expect(url.getAttribute('aria-description')).toContain('漫畫獎');
    for (const key of ['title', 'date']) {
      expect(bar.querySelector(`[data-key="${key}"]`)!.classList.contains('chip--diagnostic'))
        .toBe(false);
    }
  });

  // A stale mark is worse than no mark: it accuses a field in a file that
  // never had the problem.
  test('a clean file clears the previous file’s marks', async () => {
    const bar = chipBar('url');
    validate.mockResolvedValue({
      status: 'ok', data: [diag('url', 'taken')],
    } as never);
    await d.load('/vault/a.md');
    d.decorate(bar);
    expect(bar.querySelector<HTMLElement>('[data-key="url"]')!.dataset.tooltip).toBe('taken');

    validate.mockResolvedValue({ status: 'ok', data: [] } as never);
    await d.load('/vault/b.md');
    d.decorate(bar);

    const url = bar.querySelector<HTMLElement>('[data-key="url"]')!;
    expect(url.classList.contains('chip--diagnostic')).toBe(false);
    expect(url.dataset.tooltip).toBeUndefined();
    expect(url.hasAttribute('aria-description')).toBe(false);
  });

  test('severity 1 gets the stronger mark, severity 2 does not', async () => {
    validate.mockResolvedValue({
      status: 'ok',
      data: [diag('title', "required field 'title' is missing", 1), diag('url', 'taken', 2)],
    } as never);

    const bar = chipBar('title', 'url');
    await d.load('/vault/a.md');
    d.decorate(bar);

    expect(bar.querySelector('[data-key="title"]')!.classList.contains('chip--error')).toBe(true);
    expect(bar.querySelector('[data-key="url"]')!.classList.contains('chip--error')).toBe(false);
  });

  // The editor must not break around a surface that is only advisory.
  test('a failing or erroring validate leaves every chip unmarked', async () => {
    const bar = chipBar('url');
    validate.mockRejectedValue(new Error('no project open'));
    await expect(d.load('/vault/a.md')).resolves.toBeUndefined();
    d.decorate(bar);
    expect(bar.querySelector('[data-key="url"]')!.classList.contains('chip--diagnostic')).toBe(false);

    validate.mockResolvedValue({ status: 'error', error: 'x' } as never);
    await d.load('/vault/a.md');
    d.decorate(bar);
    expect(d.for('url')).toBeNull();
  });

  test('a diagnostic with no field addresses no chip', async () => {
    validate.mockResolvedValue({
      status: 'ok', data: [diag(null, 'something site-wide')],
    } as never);
    const bar = chipBar('title', 'url');
    await d.load('/vault/a.md');
    d.decorate(bar);
    expect(bar.querySelectorAll('.chip--diagnostic').length).toBe(0);
  });
});
