import { describe, test, expect } from 'vitest';
import { createGenerationToken } from '../generation-token';

describe('createGenerationToken', () => {
  test('a read value is stale once bump() runs, fresh again once re-read', () => {
    const gen = createGenerationToken();
    const before = gen.current();
    expect(gen.isStale(before)).toBe(false);
    gen.bump();
    expect(gen.isStale(before)).toBe(true);
    expect(gen.isStale(gen.current())).toBe(false);
  });

  test('current() reflects the live value bump() advanced to', () => {
    const gen = createGenerationToken();
    expect(gen.current()).toBe(0);
    expect(gen.bump()).toBe(1);
    expect(gen.current()).toBe(1);
  });
});
