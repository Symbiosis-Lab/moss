if (document.querySelector('moss-stage')) {
  const stageModule = new URL('../../ui/stage/moss-stage.js', window.mossTheme.base);
  const sceneModule = new URL('../../ui/stage/moss-scene.js', window.mossTheme.base);
  // Sequenced, not parallel: a marker's click dispatches `moss-scene-play`, which only reaches
  // the stage if the stage's document-level listener is already attached — so moss-stage.js must
  // finish defining its element, and therefore attaching that listener, before moss-scene.js can
  // upgrade any marker.
  import(stageModule.href)
    .then(() => import(sceneModule.href))
    .catch((error) => {
      console.error('Could not load the moss stage', error);
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

// Authors write a plain link at the end of a paragraph or list item — `[▶](#scene=tree)`, or
// `[▶](#scene=)` for "just open the surface" with no scripted scene (site/ui/demo/README.md,
// "Markdown markers"). This upgrades every such link into a <moss-demo-marker> and, if the
// article has any, inserts the one demo frame it drives after the article's first paragraph.
// `demo.css` hides an unupgraded `a[href^="#scene="]` outright, so a reader without JavaScript
// never sees a dead ▶ that does nothing (verified: the rule is removed only by the anchor itself
// being replaced below, never by a second toggle that could fall out of sync with it).
const sceneLinks = [...document.querySelectorAll('article a[href^="#scene="]')];
if (sceneLinks.length > 0) {
  const demoFrameModule = new URL('../../ui/demo/demo-frame.js', window.mossTheme.base);
  const editorDemoModule = new URL('../../ui/demo/moss-editor-demo.js', window.mossTheme.base);
  const markerModule = new URL('../../ui/demo/moss-demo-marker.js', window.mossTheme.base);
  // Sequenced, not parallel: a marker's click dispatches `moss-demo-play`, which only reaches a
  // frame if the frame's document-level listener is already attached — so demo-frame.js (via
  // moss-editor-demo.js, which defines the element) must finish attaching that listener before
  // moss-demo-marker.js can upgrade any link into a marker that might fire before the reader even
  // finishes loading the page.
  import(demoFrameModule.href)
    .then(() => import(editorDemoModule.href))
    .then(() => import(markerModule.href))
    .then(() => {
      const article = document.querySelector('article');
      for (const link of sceneLinks) {
        const name = decodeURIComponent(link.getAttribute('href').slice('#scene='.length));
        const marker = document.createElement('moss-demo-marker');
        marker.setAttribute('name', name);
        link.replaceWith(marker);
      }
      // One surface per page today (site/ui/demo/README.md, "Surfaces") — every scene a page
      // links is checked by scripts/check-demo-scenes.mjs to name the same surface, so inserting
      // the one editor-surface frame unconditionally is correct without first fetching every
      // linked scene's own JSON just to ask which surface it wants.
      if (article && !article.querySelector('moss-editor-demo')) {
        const frame = document.createElement('moss-editor-demo');
        const firstParagraph = article.querySelector('p');
        if (firstParagraph) firstParagraph.after(frame);
        else article.prepend(frame);
      }
    })
    .catch((error) => {
      console.error('Could not load the moss demo frame', error);
    });
}
