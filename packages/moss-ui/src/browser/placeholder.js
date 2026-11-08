const status = document.querySelector('#status');
fetch('/__moss/config').then(response => {
  if (!response.ok) throw new Error('Unable to connect');
  return response.json();
}).then(config => {
  status.textContent = 'The browser editor is coming soon. Your preview is ready.';
  const preview = document.createElement('a');
  preview.href = config.preview_url;
  preview.textContent = 'Open preview';
  status.after(preview);
}).catch(() => { status.textContent = 'Unable to connect to moss.'; });
