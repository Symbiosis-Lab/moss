import { describe, test, expect } from 'vitest'
import { effectiveChromeTheme, relativeLuminance } from '../chrome-theme'

describe('relativeLuminance', () => {
  test('parses #rrggbb', () => {
    expect(relativeLuminance('#ffffff')).toBeCloseTo(1, 2)
    expect(relativeLuminance('#000000')).toBeCloseTo(0, 2)
  })
  test('parses #rgb shorthand', () => {
    expect(relativeLuminance('#fff')).toBeCloseTo(1, 2)
  })
  test('parses rgb() and rgba()', () => {
    expect(relativeLuminance('rgb(255,255,255)')).toBeCloseTo(1, 2)
    expect(relativeLuminance('rgba(0, 0, 0, 1)')).toBeCloseTo(0, 2)
  })
  test('returns null for unparseable (hsl, named, garbage)', () => {
    expect(relativeLuminance('hsl(30 12% 97%)')).toBeNull()
    expect(relativeLuminance('rebeccapurple')).toBeNull()
    expect(relativeLuminance('')).toBeNull()
  })
  test('returns null for percentage rgb (NaN channels)', () => {
    expect(relativeLuminance('rgb(100% 50% 0%)')).toBeNull()
  })
})

describe('effectiveChromeTheme', () => {
  test('cream backdrop -> light chrome (dark glyphs)', () => {
    expect(effectiveChromeTheme('#faf8f5', null)).toBe('light')
    expect(effectiveChromeTheme('#F4EDE3', null)).toBe('light')
  })
  test('dark backdrop -> dark chrome (light glyphs)', () => {
    expect(effectiveChromeTheme('#1a1816', null)).toBe('dark')
    expect(effectiveChromeTheme('#111111', null)).toBe('dark')
  })
  test('light gray outside dead-band -> light directly', () => {
    // #aaaaaa luminance ~0.40 > 0.239 (THRESHOLD 0.179 + DEAD_BAND 0.06) -> light
    expect(effectiveChromeTheme('#aaaaaa', null)).toBe('light')
  })
  test('null/unparseable -> null (caller mirrors intended theme)', () => {
    expect(effectiveChromeTheme(null, 'dark')).toBeNull()
    expect(effectiveChromeTheme('hsl(0 0% 0%)', 'light')).toBeNull()
  })
  test('hysteresis: inside dead-band keeps previous', () => {
    // L(#767676) ~ 0.181, inside dead-band [0.119, 0.239] -> keep previous
    expect(effectiveChromeTheme('#767676', 'dark')).toBe('dark')
    expect(effectiveChromeTheme('#767676', 'light')).toBe('light')
  })
  test('inside dead-band with no previous -> luminance tie-break', () => {
    // L(#767676) ~ 0.181 >= 0.179 -> light when previous is null
    expect(effectiveChromeTheme('#767676', null)).toBe('light')
  })
})
