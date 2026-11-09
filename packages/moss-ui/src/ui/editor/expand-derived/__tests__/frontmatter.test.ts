import { describe, expect, it } from 'vitest';
import { headFor, patchBetween, splitPage } from '../frontmatter';

describe('headFor', () => {
  it('is empty when nothing is inferred, so the backend has nothing to write', () => {
    expect(headFor({ title: 'Mine' })).toBe('');
    expect(headFor({ inferred: [] })).toBe('');
  });
  it('carries inferred and an own title, quoted as flow scalars', () => {
    expect(headFor({ inferred: ['title', 'description'], title: 'A "q"' }))
      .toBe('---\ninferred: ["title", "description"]\ntitle: "A \\"q\\""\n---\n');
  });
});

describe('splitPage and patchBetween', () => {
  const sent = headFor({ inferred: ['title', 'description'] });
  const got = '---\ninferred: ["description"]\ntitle: Summary of 2019\n---\n';
  it('splits the rewritten head from the body, whatever the body holds', () => {
    expect(splitPage(sent, `${got}---\nfake rule above\n`)).toEqual({ head: got, body: '---\nfake rule above\n' });
    expect(splitPage('', 'text\n')).toEqual({ head: '', body: 'text\n' });
  });
  it('reports the written title and the shortened list', () => {
    expect(patchBetween(sent, got)).toEqual({ title: 'Summary of 2019', inferred: ['description'] });
  });
  it('carries an own description and reports a written one beside the title', () => {
    expect(headFor({ inferred: ['title', 'description'], description: 'Mine' }))
      .toBe('---\ninferred: ["title", "description"]\ndescription: "Mine"\n---\n');
    const both = '---\ntitle: Summary\ndescription: The harvest rose.\n---\n';
    expect(patchBetween(sent, both)).toEqual({ title: 'Summary', description: 'The harvest rose.', inferred: null });
  });
  it('reports an emptied list as a removal, and a quoted title as its value', () => {
    expect(patchBetween(sent, "---\ntitle: 'It''s: here'\n---\n")).toEqual({ title: "It's: here", inferred: null });
  });
  it('reports nothing when the head came back as sent', () => {
    expect(patchBetween(sent, sent)).toEqual({});
  });
  it('reports a written date and removes inferred when all three facts were taken', () => {
    const all = headFor({ inferred: ['title', 'description', 'date'] });
    expect(all).toBe('---\ninferred: ["title", "description", "date"]\n---\n');
    const written = '---\ntitle: Summary\ndescription: The harvest rose.\ndate: 2019-03-04\n---\n';
    expect(patchBetween(all, written)).toEqual({ title: 'Summary', description: 'The harvest rose.', date: '2019-03-04', inferred: null });
  });
  it('carries an author date so it is never reported as written', () => {
    const own = headFor({ inferred: ['date'], date: '2019-01-01' });
    expect(own).toBe('---\ninferred: ["date"]\ndate: "2019-01-01"\n---\n');
    expect(patchBetween(own, '---\ndate: "2019-01-01"\n---\n')).toEqual({ inferred: null });
  });
});
