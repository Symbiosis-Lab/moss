(() => {
  // The subscriber's newsletter is the language they signed up in: the root
  // (English) is scope '', each language folder is its own scope.
  const subscriptionScope = () => {
    const locale = document.documentElement.lang;
    return locale === 'zh-hans' || locale === 'zh-hant' ? locale : '';
  };
  const ready = (fn) => document.readyState === 'loading'
    ? document.addEventListener('DOMContentLoaded', fn, { once: true })
    : fn();

  ready(() => {
    // Not `closingWired`: older cached pages show their form on that flag, and nothing here would submit it.
    if (document.documentElement.dataset.landingWired === 'true') return;
    document.documentElement.dataset.landingWired = 'true';
    const downloads = document.querySelector('#downloads');
    const icon = { macos: 'apple.svg', windows: 'windows.svg', linux: 'linux.svg' };

    if (downloads) {
      for (const link of downloads.querySelectorAll('[data-platform]')) {
        const platform = link.dataset.platform;
        const img = document.createElement('img');
        img.src = `assets/platforms/${icon[platform]}`;
        img.alt = '';
        img.setAttribute('aria-hidden', 'true');
        link.prepend(img);
      }
    }

    const commands = document.querySelector('#commands');
    // The messages are in the markup; this only shows the one that applies.
    const say = (which) => document.querySelectorAll('#copy-status [data-status]').forEach((m) => { m.hidden = m.dataset.status !== which; });
    const selectCommand = (button) => {
      const code = button.closest('.command')?.querySelector('code');
      if (!code) return;
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(code);
      selection.removeAllRanges();
      selection.addRange(range);
      code.scrollIntoView({ block: 'nearest' });
    };
    const copy = async (button) => {
      const value = button.dataset.copy || '';
      let copied = false;
      try {
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(value);
          copied = true;
        }
      } catch (_) {}
      if (!copied) {
        selectCommand(button);
        try { copied = document.execCommand('copy') === true; } catch (_) {}
      }
      if (copied) {
        button.dataset.copied = 'true';
        say('copied');
        setTimeout(() => {
          delete button.dataset.copied;
          say(null);
        }, 1600);
      } else {
        selectCommand(button);
        say('selected');
      }
    };
    commands?.addEventListener('click', (event) => {
      const button = event.target.closest('[data-copy]');
      if (button) copy(button);
    });

    // The sign-up itself is moss's subscribe form (home/subscribe.js); the
    // landing only says which newsletter the reader joins.
    const scope = document.querySelector('#beta-form input[name="scope"]');
    if (scope) scope.value = subscriptionScope();
    if (window.mossSubscribe) document.documentElement.dataset.signupWired = 'true';
    // The visit counter counts a click on [data-track]; on success the button
    // only resets the form, so that click is not a sign-up.
    const form = document.querySelector('#beta-form');
    const button = form?.querySelector('button[type="submit"]');
    if (form && button) {
      const track = button.dataset.track;
      form.addEventListener('click', () => {
        if (form.dataset.state !== 'success') return;
        delete button.dataset.track;
        setTimeout(() => { button.dataset.track = track; });
      }, true);
    }

    document.querySelectorAll('a[href="#downloads"]').forEach((link) => {
      link.addEventListener('click', (event) => {
        event.preventDefault();
        const downloads = document.querySelector('#downloads');
        downloads?.scrollIntoView({ behavior: 'auto', block: 'center' });
        window.mossLanding?.openSignup();
        const started = performance.now();
        // The buttons live inside the staged closing scene, so they take the pointer only once it
        // has settled: the focus waits for the scene, not for the scroll.
        const focusDownload = () => {
          const state = window.__state?.();
          const first = downloads?.querySelector('a');
          if (first && !first.closest('[inert]') && state?.shown === 4 && !state.running) first.focus({ preventScroll: true });
          else if (performance.now() - started < 10000) setTimeout(focusDownload, 50);
        };
        focusDownload();
        if ('onscrollend' in window) addEventListener('scrollend', focusDownload, { once: true });
      });
    });
  });
})();
