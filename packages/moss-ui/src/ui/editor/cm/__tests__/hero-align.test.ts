import { describe, expect, test } from 'vitest';
import { heroLineWithAlign } from '../hero-align';
import { heroAttrs } from '../hero-opener';

describe('heroLineWithAlign', () => {
  test.each([
    [':::hero', 'end', ':::hero {align=end}'],
    [':::hero', 'center', ':::hero {align=center}'],
    [':::hero', 'start', ':::hero'],
    [':::hero {image=a.jpg}', 'end', ':::hero {image=a.jpg align=end}'],
    [':::hero {.plate}', 'center', ':::hero {.plate align=center}'],
    [':::hero {align=end}', 'center', ':::hero {align=center}'],
    [':::hero {align=center image=a.jpg}', 'end', ':::hero {align=end image=a.jpg}'],
    [':::hero {image=a.jpg align=end}', 'start', ':::hero {image=a.jpg}'],
    [':::hero {align=end image=a.jpg mobile=overlay}', 'start', ':::hero {image=a.jpg mobile=overlay}'],
    [':::hero {align=end}', 'start', ':::hero'],
    [':::hero {image=a.jpg}', 'start', ':::hero {image=a.jpg}'],
    [':::hero ./a.jpg|cover', 'end', ':::hero ./a.jpg|cover {align=end}'],
    [':::hero {align = end}', 'center', ':::hero {align=center}'],
    [':::hero {align="end"}', 'start', ':::hero'],
  ] as const)('%s → %s', (line, align, expected) => {
    expect(heroLineWithAlign(line, align)).toBe(expected);
  });

  test('other attributes are not touched, quoted text included', () => {
    const line = ':::hero {caption="align=end  and {braces}" image="a b.jpg|top"}';
    expect(heroLineWithAlign(line, 'end')).toBe(':::hero {caption="align=end  and {braces}" image="a b.jpg|top" align=end}');
    expect(heroLineWithAlign(line, 'start')).toBe(line);
  });

  test('a name that merely ends in align is not the attribute', () => {
    expect(heroLineWithAlign(':::hero {text-align=left}', 'end')).toBe(':::hero {text-align=left align=end}');
  });

  test('an attribute block that runs over several lines is left alone', () => {
    expect(heroLineWithAlign(':::hero {image=a.jpg', 'end')).toBe(':::hero {image=a.jpg');
  });

  test('reads attributes with their spans, quoted values included', () => {
    const line = ':::hero {.plate image="a b.jpg|top" align=end}';
    const { attrs, close } = heroAttrs(line);
    expect(attrs.map((a) => [a.key, a.value, line.slice(a.from, a.to)])).toEqual([
      ['image', 'a b.jpg|top', 'image="a b.jpg|top"'], ['align', 'end', 'align=end'],
    ]);
    expect(close).toBe(line.length - 1);
  });
});
