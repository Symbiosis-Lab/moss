// The scratch site the diagram render gate builds: one page per corpus folder, each diagram under
// its own heading, so a failure names the diagram.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { ScratchSiteSpec } from './scratch-site';

const CORPUS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../crates/moss-build/tests/fixtures/mermaid-corpus');

/** The corpus folders that become pages, and how many diagrams each page carries. */
export const DIAGRAM_PAGES = ['valid', 'malformed', 'adversarial'] as const;

export function corpusFiles(folder: string): string[] {
  return fs.readdirSync(path.join(CORPUS, folder)).filter((f) => f.endsWith('.mmd')).sort();
}

function page(folder: string): string {
  const blocks = corpusFiles(folder).map((f) => {
    const src = fs.readFileSync(path.join(CORPUS, folder, f), 'utf8').replace(/\n+$/, '');
    // A tilde fence, so a corpus diagram containing backticks cannot close it early.
    return `## ${f.replace(/\.mmd$/, '')}\n\nText before the diagram.\n\n~~~mermaid\n${src}\n~~~\n`;
  });
  return `---\ntitle: Diagrams, ${folder}\n---\n\n${blocks.join('\n')}`;
}

export const DIAGRAMS_GATE: ScratchSiteSpec = {
  name: 'diagrams-gate',
  files: {
    '.moss/config.toml': 'schema_version = 5\n\n[site]\nlang = "en"\n',
    'index.md': '---\ntitle: Diagrams\n---\n\nDiagram render gate.\n',
    ...Object.fromEntries(DIAGRAM_PAGES.map((f) => [`${f}.md`, page(f)])),
  },
};
