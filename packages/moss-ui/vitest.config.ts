import { defineConfig } from 'vitest/config';

// The UI's tests that touch no DOM run in node; every other UI test gets jsdom.
const UI_NODE_TESTS = [
  'src/ui/__tests__/toast-shelf-position.test.ts',
  'src/ui/i18n/__tests__/channel-name.test.ts',
  'src/ui/i18n/__tests__/interpolate.test.ts',
  'src/ui/i18n/__tests__/lang-names.test.ts',
  'src/ui/i18n/__tests__/on-locale-change.test.ts',
  'src/ui/i18n/__tests__/rust-keys-parity.test.ts',
  'src/ui/i18n/no-untranslated-leakage.test.ts',
  'src/ui/shared/ui/__tests__/guarded-async.test.ts',
  'src/ui/shared/ui/toast/__tests__/task-toast-subscriber.test.ts',
  'src/ui/styles/__tests__/pill-primitives.test.ts',
  'src/ui/styles/__tests__/severity-contrast.test.ts',
  'src/ui/utils/__tests__/chrome-theme.test.ts',
];
const UI_TESTS = ['src/ui/**/*.test.ts'];
const UI_SETUP = ['./src/ui/test-setup-ui.ts'];

export default defineConfig({
  test: {
    globals: true,
    projects: [
      { extends: true, test: { name: 'contract', environment: 'node', include: ['src/__tests__/**/*.test.ts'] } },
      { extends: true, test: { name: 'scripts', environment: 'node', include: ['scripts/__tests__/**/*.test.mjs'] } },
      { extends: true, test: { name: 'ui-node', environment: 'node', include: UI_NODE_TESTS, setupFiles: UI_SETUP } },
      {
        extends: true,
        test: { name: 'ui-dom', environment: 'jsdom', include: UI_TESTS, exclude: UI_NODE_TESTS, setupFiles: UI_SETUP },
      },
    ],
  },
});
