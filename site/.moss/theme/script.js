if (document.querySelector('moss-ui-demo')) {
  const demoModule = new URL('../../ui/moss-ui-demo.js', window.mossTheme.base);
  import(demoModule.href).catch((error) => {
    console.error('Could not load the moss interface demo', error);
  });
}

// Favicon dark-mode swap. The moss mark's SVG favicon (site/assets/brand/favicon.svg,
// same file whether it's the landing page's own <link> or the one moss's docs-page
// build rasterizes from) carries a `<style>@media (prefers-color-scheme:dark)` rule
// that inverts the ink to white/#a3d483 — but Safari (through at least 26.0) fetches
// and renders an SVG favicon's base styles only, never evaluating @media inside it, so
// Safari shows the light-mode mark regardless of OS theme. There is no reliable way to
// fix that with markup alone: Safari's support for the alternative, separate
// `<link rel="icon" media="...">` elements, is undocumented and inconsistently
// reported. So this swaps the icon links' `href` directly with a `matchMedia`
// listener — ordinary DOM behavior every engine (including Safari) implements the
// same way, sidestepping favicon-specific media handling entirely. site/assets/brand/
// carries pre-rendered dark counterparts (favicon-dark.svg/-16/-32/-180.png) for this;
// see favicon-dark.svg's own header for how they were generated.
(() => {
  const darkHref = (link) => {
    if (link.getAttribute('rel') === 'apple-touch-icon') return '/assets/brand/favicon-dark-180.png';
    if (link.type === 'image/svg+xml') return '/assets/brand/favicon-dark.svg';
    if (link.getAttribute('sizes') === '32x32') return '/assets/brand/favicon-dark-32.png';
    if (link.getAttribute('sizes') === '16x16') return '/assets/brand/favicon-dark-16.png';
    return null;
  };
  const icons = [...document.querySelectorAll('link[rel~="icon"], link[rel="apple-touch-icon"]')]
    .map((link) => ({ link, light: link.getAttribute('href'), dark: darkHref(link) }))
    .filter((entry) => entry.dark);
  const apply = (dark) => {
    for (const entry of icons) entry.link.setAttribute('href', dark ? entry.dark : entry.light);
  };
  const query = matchMedia('(prefers-color-scheme: dark)');
  apply(query.matches);
  query.addEventListener('change', (event) => apply(event.matches));
})();
