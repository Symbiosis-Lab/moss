if (document.querySelector('moss-ui-demo')) {
  const demoModule = new URL('../../ui/moss-ui-demo.js', window.mossTheme.base);
  import(demoModule.href).catch((error) => {
    console.error('Could not load the moss interface demo', error);
  });
}
