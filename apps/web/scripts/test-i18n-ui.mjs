import { chromium } from 'playwright';
import assert from 'node:assert/strict';

// French and English browser journeys against a synthetic Core: the API mock reproduces the documented
// contract (docs/METADATA_LOCALIZATION.md): `?locale=` first, translated titles, search over translated and
// original titles, preferences merged on PUT with `null` clearing a field. This checks the interface, not the
// Core (the Core has its own tests).
const base = process.env.MEDIA_WEB_TEST_URL ?? 'http://127.0.0.1:5188/app';
const launch = process.env.MEDIA_WEB_TEST_CHANNEL ? { channel: process.env.MEDIA_WEB_TEST_CHANNEL } : {};
const browser = await chromium.launch({ headless: true, ...launch });

const works = [
  { id: 'w1', title: 'Les Évadés', original: 'Rita Hayworth and Shawshank Redemption', year: 1994, tr: { en: 'The Shawshank Redemption' } },
  { id: 'w2', title: 'Alpha', original: 'Alpha', year: 2000, tr: { en: 'Zulu' } },
  { id: 'w3', title: 'Bravo', original: 'Bravo', year: 2001, tr: { en: 'Echo' } }
];
const fold = text => text.toLowerCase();

async function newSession(browserContextOptions = {}) {
  const server = { prefs: { locale: 'fr' }, puts: [], catalogUrls: [] };
  const context = await browser.newContext({ locale: 'fr-FR', ...browserContextOptions });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/api/v1/**', async route => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    let json = {};
    if (path.endsWith('/auth/me')) json = { user: { id: 'ana', login: 'ana', displayName: 'Ana', role: 'user' }, csrfToken: 'csrf' };
    else if (path.endsWith('/auth/status')) json = { initialized: true };
    else if (path.endsWith('/libraries')) json = { libraries: [] };
    else if (path.endsWith('/preferences')) {
      if (request.method() === 'PUT') {
        const patch = JSON.parse(request.postData() ?? '{}');
        server.puts.push(patch);
        for (const [key, value] of Object.entries(patch)) { if (value === null) delete server.prefs[key]; else server.prefs[key] = value; }
        return route.fulfill({ status: 204 });
      }
      json = server.prefs;
    } else if (path.endsWith('/catalog')) {
      server.catalogUrls.push(url.search);
      const wanted = (url.searchParams.get('locale') ?? 'fr').split('-')[0];
      const q = fold(url.searchParams.get('q') ?? '');
      const titleOf = work => (wanted === 'fr' ? work.title : work.tr[wanted] ?? work.title);
      const matches = work => !q || [work.title, work.original, ...Object.values(work.tr)].some(title => fold(title).includes(q));
      const items = works.filter(matches).map(work => ({ id: work.id, kind: 'film', title: titleOf(work), year: work.year, description: '', libraryId: 'lib', synthetic: false, editions: [], posterUrl: null }))
        .sort((a, b) => a.title.localeCompare(b.title, wanted));
      json = { items, total: items.length, hasMore: false, offset: 0, limit: 24, notice: '' };
    } else if (path.endsWith('/progress')) json = { items: [], total: 0 };
    else if (path.endsWith('/server')) json = { name: 'Synthetic' };
    else if (path.endsWith('/playback/statistics')) json = { canView: false };
    await route.fulfill({ json });
  });
  return { server, context, page, errors };
}

async function until(condition, message) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (condition()) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.fail(message);
}

const rawKey = /\b[a-z][a-z0-9]*(?:_[a-z0-9]+){2,}\b/;
async function assertNoRawKeys(page, label) {
  const text = await page.locator('body').innerText();
  const leaked = text.split(/\s+/).filter(word => rawKey.test(word));
  assert.deepEqual(leaked, [], `${label}: untranslated keys on screen`);
}
async function assertNamedControls(page, label) {
  const unnamed = await page.evaluate(() => [...document.querySelectorAll('button, a[href], input:not([type=hidden]), select, textarea')]
    .filter(element => !element.closest('[hidden]') && element.getClientRects().length)
    .filter(element => {
      const labelled = element.getAttribute('aria-label') || element.getAttribute('title') || element.textContent?.trim() || element.getAttribute('placeholder')
        || element.labels?.[0]?.textContent?.trim() || (element.getAttribute('aria-labelledby') && document.getElementById(element.getAttribute('aria-labelledby'))?.textContent?.trim());
      return !labelled;
    }).map(element => element.outerHTML.slice(0, 120)));
  assert.deepEqual(unnamed, [], `${label}: controls without an accessible name`);
}

try {
  // 1. French journey: interface, titles and search.
  {
    const { page, server, errors } = await newSession();
    await page.goto(`${base}/films/`);
    await page.getByRole('link', { name: 'Accueil' }).first().waitFor();
    assert.equal(await page.locator('html').getAttribute('lang'), 'fr');
    await page.getByText('Les Évadés').first().waitFor();
    assert.ok(server.catalogUrls.every(search => new URLSearchParams(search).get('locale') === 'fr'), 'catalogue requests name the metadata language');
    await assertNoRawKeys(page, 'fr films');
    await assertNamedControls(page, 'fr films');

    // Search finds a work by a translated title, an original title and the library title, and shows it once.
    for (const [query, expected] of [['redemption', 'Les Évadés'], ['rita hayworth', 'Les Évadés'], ['zulu', 'Alpha'], ['évadés', 'Les Évadés']]) {
      await page.goto(`${base}/search/?q=${encodeURIComponent(query)}`);
      await page.getByText(expected, { exact: true }).first().waitFor();
      // The same work is never listed twice, whichever of its titles matched.
      const cards = await page.evaluate(() => [...new Set([...document.querySelectorAll('a[href*="work="]')].map(link => new URL(link.href).searchParams.get('work')))]);
      assert.equal(cards.length, 1, `${query}: one work, no duplicate (${cards.join(',')})`);
    }
    assert.deepEqual(errors, []);
  }

  // 2. Hot switch to English keeps the page, the scroll context and the document (no reload).
  {
    const { page, server, errors } = await newSession();
    await page.goto(`${base}/settings/#languages`);
    await page.getByRole('heading', { name: 'Langues', exact: true }).first().waitFor();
    await page.evaluate(() => { window.__marker = 'same-document'; });
    const navigations = [];
    page.on('framenavigated', frame => { if (frame === page.mainFrame()) navigations.push(frame.url()); });
    await page.getByLabel('Langue de l’interface').last().selectOption('en');
    await page.getByRole('heading', { name: 'Languages', exact: true }).first().waitFor();
    assert.equal(await page.evaluate(() => window.__marker), 'same-document', 'the language switch must not reload the document');
    assert.equal(await page.locator('html').getAttribute('lang'), 'en');
    assert.deepEqual(server.puts.at(-1), { locale: 'en' });
    await assertNoRawKeys(page, 'en settings');
    await assertNamedControls(page, 'en settings languages');

    // Titles follow the interface language until a metadata language is chosen on its own.
    await page.getByRole('link', { name: 'Movies' }).first().click();
    await page.getByText('The Shawshank Redemption').first().waitFor();
    assert.equal(await page.evaluate(() => window.__marker), 'same-document');
    assert.equal(new URLSearchParams(server.catalogUrls.at(-1)).get('locale'), 'en');

    // Metadata language chosen separately: menus stay English, titles come back in French.
    await page.goto(`${base}/settings/#languages`);
    await page.getByLabel('Language of titles, summaries and taglines').selectOption('fr');
    await page.waitForFunction(() => document.querySelector('.status')?.textContent?.includes('Saved'));
    assert.deepEqual(server.puts.at(-1), { metadataLocale: 'fr' });
    await page.getByRole('link', { name: 'Movies' }).first().click();
    await page.getByText('Les Évadés').first().waitFor();
    assert.equal(new URLSearchParams(server.catalogUrls.at(-1)).get('locale'), 'fr');
    assert.equal(await page.locator('html').getAttribute('lang'), 'en');

    // Fallbacks, then back to "same as the interface" clears the preference.
    await page.goto(`${base}/settings/#languages`);
    await page.getByLabel('Add a fallback language').selectOption('en');
    await page.getByRole('button', { name: 'Add', exact: true }).click();
    await page.getByText('1. English').waitFor();
    assert.deepEqual(server.puts.at(-1), { metadataFallbackLocales: ['en'] });
    await page.getByLabel('Language of titles, summaries and taglines').selectOption('');
    await page.waitForFunction(() => document.querySelector('.status')?.textContent?.includes('Saved'));
    assert.deepEqual(server.puts.at(-1), { metadataLocale: null });
    assert.equal('metadataLocale' in server.prefs, false);
    assert.deepEqual(errors, []);
  }

  // 3. Preferences survive a new session: the server value wins over the browser default.
  {
    const first = await newSession();
    first.server.prefs = { locale: 'en', metadataLocale: 'ja', artworkLocale: 'fr', subtitleLanguage: 'en' };
    await first.page.goto(`${base}/films/`);
    await first.page.getByRole('link', { name: 'Home' }).first().waitFor();
    assert.equal(await first.page.locator('html').getAttribute('lang'), 'en');
    await until(() => new URLSearchParams(first.server.catalogUrls.at(-1) ?? '').get('locale') === 'ja', 'the catalogue is requested in the saved metadata language');
    await first.page.goto(`${base}/settings/#languages`);
    assert.equal(await first.page.getByLabel('Language of titles, summaries and taglines').inputValue(), 'ja');
    assert.equal(await first.page.getByLabel('Language of posters, backdrops and logos').inputValue(), 'fr');
    assert.equal(await first.page.getByLabel('Default subtitle language').inputValue(), 'en');
    // "Reconnection": storage emptied, French browser, same account on the server.
    await first.page.evaluate(() => { localStorage.clear(); sessionStorage.clear(); });
    await first.page.reload();
    await first.page.getByRole('heading', { name: 'Languages', exact: true }).first().waitFor();
    assert.equal(await first.page.locator('html').getAttribute('lang'), 'en');
    assert.deepEqual(first.errors, []);
  }

  // 4. Two accounts never share a cached page (the second sees its own language chain, not the first's).
  {
    const { page, server } = await newSession();
    await page.goto(`${base}/films/`);
    await page.getByText('Les Évadés').first().waitFor();
    server.prefs = { locale: 'en' };
    await page.evaluate(() => { localStorage.clear(); });
    await page.reload();
    await page.getByText('The Shawshank Redemption').first().waitFor();
    assert.equal(await page.getByText('Les Évadés').count(), 0, 'no French page leaks into the English session');
  }
  console.log('i18n browser journeys passed');
} finally {
  await browser.close();
}
