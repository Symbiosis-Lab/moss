(() => {
  const ready = (fn) => document.readyState === 'loading'
    ? document.addEventListener('DOMContentLoaded', fn, { once: true })
    : fn();

  ready(() => {
    if (document.documentElement.dataset.closingWired === 'true') return;
    document.documentElement.dataset.closingWired = 'true';
    const downloads = document.querySelector('#downloads');
    const beta = document.querySelector('#beta');
    const release = {
      macos: 'https://github.com/Symbiosis-Lab/moss/releases/download/v0.14.1/moss_0.14.1_universal.dmg',
      github: 'https://github.com/Symbiosis-Lab/moss',
    };
    const icon = { macos: 'apple.svg', windows: 'windows.svg', linux: 'linux.svg', github: 'github.svg' };
    const platformLabel = { macos: 'macOS', windows: 'Windows', linux: 'Linux', github: 'GitHub' };

    if (downloads) {
      for (const link of downloads.querySelectorAll('[data-platform]')) {
        const platform = link.dataset.platform;
        const label = platformLabel[platform];
        const img = document.createElement('img');
        img.src = `assets/platforms/${icon[platform]}`;
        img.alt = '';
        img.setAttribute('aria-hidden', 'true');
        link.replaceChildren(img, document.createTextNode(label));

        if (platform === 'macos') {
          link.href = release.macos;
          link.title = 'Download moss 0.14.1 for macOS (universal DMG)';
        } else if (platform === 'github') {
          link.href = release.github;
          link.title = 'View moss on GitHub';
        } else {
          const availability = document.createElement('span');
          availability.className = 'availability';
          availability.textContent = 'Coming soon';
          link.append(availability);
          link.href = '#beta';
          link.title = `${label} desktop app — Coming soon`;
          link.setAttribute('aria-label', `${label} desktop app, Coming soon. Request beta access.`);
          link.dataset.availability = 'coming-soon';
          link.addEventListener('click', () => {
            requestAnimationFrame(() => beta?.focus({ preventScroll: true }));
          });
        }
      }
      const note = document.querySelector('#download-note');
      if (note) note.textContent = 'macOS 11 or later · Universal build for Apple silicon and Intel';
    }

    const commands = document.querySelector('#commands');
    const commandValues = {
      npm: 'npm install -g @symbiosis-lab/moss',
      brew: 'brew install --cask symbiosis-lab/tap/moss',
    };
    if (commands) {
      const rows = [...commands.querySelectorAll('.command')];
      if (rows[0]) {
        rows[0].querySelector('span').textContent = 'npm · CLI';
        rows[0].querySelector('code').textContent = commandValues.npm;
        rows[0].querySelector('[data-copy]').dataset.copy = commandValues.npm;
      }
      if (rows[1]) {
        rows[1].querySelector('span').textContent = 'Homebrew · desktop';
        rows[1].querySelector('code').textContent = commandValues.brew;
        rows[1].querySelector('[data-copy]').dataset.copy = commandValues.brew;
        rows[1].querySelector('[data-copy]').setAttribute('aria-label', 'Copy Homebrew desktop install command');
      }
    }

    const copyStatus = document.querySelector('#copy-status');
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
        copyStatus && (copyStatus.textContent = 'Copied to clipboard.');
        setTimeout(() => { delete button.dataset.copied; }, 1600);
      } else {
        selectCommand(button);
        copyStatus && (copyStatus.textContent = 'Command selected. Press Command-C or Ctrl-C to copy.');
      }
    };
    commands?.addEventListener('click', (event) => {
      const button = event.target.closest('[data-copy]');
      if (button) copy(button);
    });

    const submit = (form, request, successMessage) => {
      if (!form) return;
      const button = form.querySelector('button[type="submit"]');
      const input = form.querySelector('input[type="email"]');
      const status = form.querySelector('.form-status');
      const initialLabel = button.textContent;
      form.addEventListener('submit', async (event) => {
        event.preventDefault();
        if (!form.reportValidity() || button.disabled) return;
        button.disabled = true;
        button.setAttribute('aria-busy', 'true');
        button.textContent = 'Sending…';
        status.textContent = '';
        delete status.dataset.state;
        try {
          const response = await request(input.value.trim());
          if (!response.ok) throw new Error(`Request failed (${response.status})`);
          let payload;
          if (form.id === 'newsletter-form') {
            payload = await response.json();
            if (!payload || typeof payload !== 'object' || Array.isArray(payload) || typeof payload.alreadySubscribed !== 'boolean') {
              throw new Error('The subscription service returned an unexpected response.');
            }
          }
          form.reset();
          status.textContent = typeof successMessage === 'function' ? successMessage(payload) : successMessage;
          status.dataset.state = 'success';
        } catch (error) {
          status.textContent = error instanceof TypeError
            ? 'Could not connect. Check your connection and try again.'
            : 'Something went wrong. Please try again.';
          status.dataset.state = 'error';
          input.focus();
        } finally {
          button.disabled = false;
          button.removeAttribute('aria-busy');
          button.textContent = initialLabel;
        }
      });
    };

    submit(
      document.querySelector('#beta-form'),
      (email) => fetch('https://api.mosspub.com/apply?lang=en', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded;charset=UTF-8' },
        body: new URLSearchParams({ email, website: '', scope: '' }),
      }),
      'Thanks for applying. Confirm the email we just sent; we’ll email you when your turn comes.'
    );
    submit(
      document.querySelector('#newsletter-form'),
      (email) => fetch('https://api.mosspub.com/api/sites/landing/subscribe', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify({ email }),
      }),
      ({ alreadySubscribed }) => alreadySubscribed
        ? 'You’re already subscribed.'
        : 'Check your email to confirm your subscription.'
    );
  });
})();
