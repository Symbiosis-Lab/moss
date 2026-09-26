import { expect, test } from '@playwright/test';

test('updates the full-page link while the embedded iframe keeps its camera state', async ({ page }) => {
  await page.goto('/playwright/fixtures/iframe-open-url/live.html');

  const iframe = page.locator('iframe');
  const embeddedUrl = await iframe.getAttribute('src');
  const frame = page.frameLocator('iframe');
  const childName = () =>
    frame.locator('body').evaluate((body) => body.ownerDocument.defaultView?.name);
  const openLink = page.locator('.immersive-new-window-btn');
  await expect(openLink).toHaveAttribute('href', /camera=home/);
  await expect.poll(childName).toBe('camera:home');

  await iframe.evaluate((frame) => {
    frame.dataset.openUrl = '/playwright/fixtures/iframe-open-url/map.html?camera=selected-place';
  });
  await expect(openLink).toHaveAttribute('href', /camera=selected-place/);
  await frame.locator('body').evaluate((body) => {
    body.ownerDocument.defaultView!.name = 'camera:selected-place';
  });

  await page.locator('.immersive-fullscreen-btn').click();
  await expect(page.locator('body')).toHaveClass(/immersive-fs-active/);
  await expect(iframe).toHaveAttribute('src', embeddedUrl!);
  await expect.poll(childName).toBe('camera:selected-place');
  await expect(openLink).toHaveAttribute('href', /camera=selected-place/);
});

test('rejects non-http open targets and falls back to the iframe URL', async ({ page }) => {
  await page.goto('/playwright/fixtures/iframe-open-url/live.html');
  const iframe = page.locator('iframe');
  const openLink = page.locator('.immersive-new-window-btn');

  await iframe.evaluate((frame) => {
    frame.dataset.openUrl = 'javascript:alert(1)';
  });
  await expect(openLink).toHaveAttribute('href', /camera=home/);
});
