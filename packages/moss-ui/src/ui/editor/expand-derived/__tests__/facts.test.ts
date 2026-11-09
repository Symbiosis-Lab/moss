import { describe, it, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import { expandDerivedExtension } from '../extension';

// The probe asks the backend about the page once per open and hands the title
// and description the embeds give to the chip bar, whatever the page already sets.
async function probe(values: Record<string, unknown>, answer: string) {
  const sent: string[] = [];
  const onFacts = vi.fn();
  const view = new EditorView({
    state: EditorState.create({
      doc: '![[a.pdf#text]]',
      extensions: [markdown({ extensions: [wikilinkConfig] }), expandDerivedExtension({
        label: () => 'Expand',
        getFrontmatter: () => values,
        onFacts,
        expand: async (content) => { sent.push(content); return { content: answer, replaced: 1 }; },
      })],
    }),
  });
  await vi.waitFor(() => expect(onFacts).toHaveBeenCalled());
  view.destroy();
  return { sent: sent[0], facts: onFacts.mock.calls[0][0] };
}

describe('derived facts for the chip bar', () => {
  it('are reported from the page head the backend returns', async () => {
    const r = await probe({ inferred: ['title', 'description'] },
      '---\ninferred: ["description"]\ntitle: Summary of 2019\n---\nBody');
    expect(r.facts).toEqual({ title: 'Summary of 2019', description: undefined, date: undefined });
  });

  it('carry the derived date', async () => {
    const r = await probe({ inferred: ['date'] }, '---\ninferred: []\ndate: 2019-03-04\n---\nBody');
    expect(r.facts.date).toBe('2019-03-04');
  });

  it('are asked without the author’s own title, so a page that overrides still reports them', async () => {
    const r = await probe({ inferred: ['title'], title: 'Mine' },
      '---\ninferred: []\ntitle: Summary\n---\nBody');
    expect(r.sent).not.toContain('Mine');
    expect(r.facts.title).toBe('Summary');
  });
});
