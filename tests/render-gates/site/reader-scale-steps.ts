// The reader's Aa control applies one of these classes to <html> (site.css
// `html.scale-*`) — pure CSS, so a fixture that sets the class directly
// exercises the real rule without needing the site JS that normally writes
// it. Shared by every render gate that walks the four steps against
// stylesheets injected straight into `page.setContent` — currently
// prose-spacing-ladder.spec.ts and vertical-measure.spec.ts.
export const READER_SCALE_STEPS: Record<string, string> = {
  default: '',
  small: 'scale-small',
  large: 'scale-large',
  xlarge: 'scale-xlarge',
};

// Ratio tolerance for a gate that divides a measured px gap or size by a
// calc()-derived unit (one line of body text, a font-size): chromium and
// webkit round subpixel lengths slightly differently.
export const READER_SCALE_TOLERANCE = 0.03;
