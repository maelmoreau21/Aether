import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';

// Synthetic API/images: verifies browser rendering and interaction, not provider network access.
const origin = process.env.MEDIA_TEST_ORIGIN ?? 'http://127.0.0.1:5192';
const output = process.env.MEDIA_REPORT_DIR ?? '../../target/artwork-rotation-ui';
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage();
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  let token = 'synthetic-session-one';
  const images = ['poster', 'backdrop', 'logo', 'thumb', 'illustration'].flatMap(kind => ['a', 'b'].map(suffix => ({ id: `${kind}-${suffix}`, kind, origin: 'provider', provider: 'tmdb', position: 0, language: 'fr', url: `/api/v1/works/film/images/${kind}-${suffix}` })));
  const work = { id: 'film', kind: 'film', title: 'Images renouvelées', year: 2026, description: 'Fixture créée pour ce test.', libraryId: 'films', synthetic: false, editions: [], posterUrl: images[0].url, artwork: { backdrop: images[2].url }, imageVariants: images };
  let collected;
  await page.context().route('**/api/v1/**', async route => {
    const request = route.request(); const url = new URL(request.url()); const path = url.pathname;
    if (/\/works\/film\/images\//.test(path)) {
      await route.fulfill({ contentType: 'image/png', body: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aZ1sAAAAASUVORK5CYII=', 'base64') }); return;
    }
    let json = {};
    if (path.endsWith('/auth/me')) json = { user: { id: 'test-admin', login: 'admin', role: 'administrator' }, csrfToken: token };
    else if (path.endsWith('/auth/status')) json = { initialized: true };
    else if (path.endsWith('/preferences')) json = { locale: 'fr', theme: 'dark' };
    else if (path.endsWith('/catalog/film')) json = work;
    else if (path.endsWith('/catalog')) json = { items: url.searchParams.get('kind') === 'series' ? [] : [work], total: 1, offset: 0, limit: 24 };
    else if (path.endsWith('/works/film/images')) json = { images };
    else if (path.endsWith('/extensions/images/collect')) {
      assert.equal(request.headers()['x-csrf-token'], token); collected = request.postDataJSON();
      json = { downloaded: 2, duplicates: 1, skipped: 0, errors: [], partial: true };
    }
    else if (path.endsWith('/libraries')) json = { libraries: [{ id: 'films', name: 'Films', kind: 'films' }] };
    else if (path.endsWith('/progress')) json = { items: [] };
    else if (path.endsWith('/lists')) json = { lists: [] };
    else if (path.includes('/feedback/')) json = { favorite: false, liked: null };
    else if (path.endsWith('/account/avatar')) { await route.fulfill({ status: 404 }); return; }
    else if (path.endsWith('/server')) json = { name: 'Synthetic' };
    await route.fulfill({ json });
  });
  const decoded = async selector => {
    await page.waitForFunction(selector => { const image = document.querySelector(selector); return image?.complete && image.naturalWidth > 0; }, selector);
    return page.locator(selector).first().getAttribute('src');
  };
  await page.goto(`${origin}/app/films/?work=film`);
  await page.getByRole('heading', { name: work.title, exact: true }).waitFor();
  const first = await decoded('.detail-poster-column .poster-image');
  const firstBackdrop = await decoded('.backdrop-art-image');
  await decoded('.title-logo'); await decoded('.artwork-gallery img');
  await page.reload();
  assert.equal(await decoded('.detail-poster-column .poster-image'), first);
  assert.equal(await decoded('.backdrop-art-image'), firstBackdrop);
  token = 'synthetic-session-two'; await page.reload();
  assert.notEqual(await decoded('.detail-poster-column .poster-image'), first);
  assert.notEqual(await decoded('.backdrop-art-image'), firstBackdrop);
  await page.getByRole('button', { name: "Plus d'actions et informations" }).click();
  await page.getByText('Modifier les images', { exact: true }).click();
  const dialog = page.getByRole('dialog', { name: `Modifier les images · ${work.title}` });
  await dialog.getByText('fr · tmdb', { exact: true }).first().waitFor();
  await dialog.getByLabel('Langues et secours, dans l’ordre').fill('fr, en');
  await dialog.getByRole('button', { name: 'Collecter les images', exact: true }).click();
  await dialog.getByText(/2 images téléchargées · 1 doublons évités/).waitFor({ timeout: 5000 }).catch(async error => { console.log({ collected, errors, dialog: await dialog.innerText().catch(() => 'closed') }); throw error; });
  assert.deepEqual(collected, { workId: 'film', languages: ['fr', 'en'], includeNeutral: true, maxImages: 12 });
  await page.screenshot({ path: `${output}/collection-desktop.png`, fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  await page.screenshot({ path: `${output}/collection-mobile.png`, fullPage: true });
  const currentPoster = await page.locator('.detail-poster-column .poster-image').getAttribute('src');
  const nextVisit = await page.context().newPage();
  await nextVisit.goto(`${origin}/app/films/?work=film`);
  await nextVisit.waitForFunction(() => { const image = document.querySelector('.detail-poster-column .poster-image'); return image?.complete && image.naturalWidth > 0; });
  assert.notEqual(await nextVisit.locator('.detail-poster-column .poster-image').getAttribute('src'), currentPoster, 'new tab must rotate even with the same authentication session');
  await nextVisit.close();
  assert.deepEqual(errors, []);
  await writeFile(`${output}/result.json`, JSON.stringify({ mockApi: true, browser: browser.version(), decodedRoles: ['poster', 'backdrop', 'logo', 'illustration'], reloadStable: true, nextSessionChanged: true, newVisitWithoutLoginChanged: true, collection: collected, pageErrors: errors }, null, 2));
  console.log('PASS: decoded artwork roles, reload stability, next-session changes, collection CSRF/body/status, mobile overflow. Mock API only.');
} finally { await browser.close(); }
