import { chromium } from 'playwright';
import assert from 'node:assert/strict';

// Synthetic 10,000-entry UI contract: this is not a NAS or real API benchmark.
const base = process.env.MEDIA_WEB_TEST_URL ?? 'http://127.0.0.1:5188/app';
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage();
  const errors = [];
  const episodeRequests = [];
  let maxEpisodePayload = 0;
  page.on('pageerror', error => errors.push(error.message));
  const entries = Array.from({ length: 10000 }, (_, index) => ({ kind: 'work', id: `work-${index}`, media: { id: `work-${index}`, title: `Film ${index + 1}`, kind: 'film' } }));
  const list = { id: 'large-list', name: '10 000 films', owned: true, visibility: 'private' };
  const episode = index => ({ id: `episode-${index}`, title: `Épisode ${index + 1}`, number: index + 1, kind: 'episode', synthetic: true, libraryId: 'series', year: null, description: '', editions: [{ id: `edition-${index}`, label: 'Local', versions: [{ id: `version-${index}`, label: 'Local', sources: [{ id: `source-${index}`, assetIds: [`media-${index}`], playbackMediaId: `media-${index}` }] }] }] });
  const series = { id: 'large-series', title: 'Grande série', kind: 'series', synthetic: true, libraryId: 'series', year: null, description: '', seasons: [{ id: 'season-1', number: 1, title: 'Saison 1', episodes: Array.from({ length: 10000 }, (_, index) => episode(index)) }, { id: 'season-2', number: 2, title: 'Saison 2', episodes: [episode(10000)] }] };
  await page.route('**/api/v1/**', async route => {
    const path = new URL(route.request().url()).pathname;
    let json = {};
    if (path.endsWith('/auth/me')) json = { user: { id: 'user', login: 'test', role: 'user' }, csrfToken: 'synthetic' };
    else if (path.endsWith('/auth/status')) json = { initialized: true };
    else if (path.endsWith('/libraries')) json = { libraries: [] };
    else if (path.endsWith('/lists/large-list')) json = { ...list, entries, shares: [] };
    else if (path.endsWith('/lists')) json = { lists: [list] };
    else if (path.endsWith('/playlists/policy')) json = { allowPublic: false, allowSharing: false };
    else if (path.endsWith('/catalog/large-series')) {
      const params = new URL(route.request().url()).searchParams;
      const after = params.get('episodeAfter');
      const afterIndex = after ? Number(after.replace('episode-', '')) : -1;
      const season = after ? (afterIndex >= 9999 ? 2 : 1) : Number(params.get('season') ?? '1');
      const offset = after ? (season === 1 ? afterIndex + 1 : Math.max(0, afterIndex - 9999)) : Number(params.get('episodeOffset') ?? '0');
      const limit = Number(params.get('episodeLimit') ?? '24');
      episodeRequests.push({ season, offset, limit, after });
      const seasons = series.seasons.map(item => ({ ...item, episodeCount: item.episodes.length, episodes: item.number === season ? item.episodes.slice(offset, offset + limit) : [] }));
      maxEpisodePayload = Math.max(maxEpisodePayload, seasons.reduce((count, item) => count + item.episodes.length, 0));
      json = { ...series, seasons, episodesPage: { season, offset, limit, nextOffset: offset + seasons.find(item => item.number === season).episodes.length, hasMore: offset + limit < series.seasons.find(item => item.number === season).episodes.length } };
    }
    else if (path.endsWith('/catalog') || path.endsWith('/progress')) json = { items: [], total: 0 };
    else if (/\/media\/media-/.test(path)) json = { id: path.split('/').pop(), title: 'Synthetic descriptor', synthetic: true, sources: [], subtitles: [] };
    else if (path.endsWith('/preferences')) json = { locale: 'fr', theme: 'dark' };
    else if (path.endsWith('/server')) json = { name: 'Synthetic' };
    await route.fulfill({ json });
  });
  await page.goto(`${base}/lists/?id=large-list`);
  await page.getByRole('heading', { name: '10 000 films', exact: true }).waitFor();
  assert.equal(await page.locator('.entries li').count(), 50);
  await page.getByRole('navigation', { name: 'Pagination des éléments' }).getByRole('button', { name: 'Suivants', exact: true }).click();
  assert.equal(await page.locator('.entries li').count(), 50);
  assert.match(await page.locator('.entries li').first().innerText(), /Film 51/);
  assert.equal(await page.locator('.entries li').first().getByRole('button', { name: 'Monter Film 51' }).isEnabled(), true);
  await page.goto(`${base}/series/?work=large-series`);
  await page.getByRole('heading', { name: 'Grande série', exact: true }).waitFor();
  assert.equal(await page.locator('.episodes-list .episode').count(), 24);
  await page.getByRole('navigation', { name: 'Pagination des épisodes' }).getByRole('button', { name: 'Suivants', exact: true }).click();
  await page.getByRole('heading', { name: 'Épisode 25', exact: true }).waitFor();
  assert.equal(await page.locator('.episodes-list .episode').count(), 24);
  assert.match(await page.locator('.episodes-list .episode').first().innerText(), /Épisode 25/);
  await page.getByLabel('Saison', { exact: true }).selectOption('2');
  await page.getByRole('heading', { name: 'Épisode 10001', exact: true }).waitFor();
  assert.equal(await page.locator('.episodes-list .episode').count(), 1);
  assert.match(await page.locator('.episodes-list .episode').first().innerText(), /Épisode 10001/);
  // This checks next-episode control flow only; empty synthetic sources do not prove decoding.
  await page.goto(`${base}/series/?work=large-series`);
  await page.getByRole('heading', { name: 'Épisode 24', exact: true }).waitFor();
  const nextRequest = page.waitForRequest(request => new URL(request.url()).searchParams.get('episodeAfter') === 'episode-23');
  await page.getByRole('button', { name: 'Lire Épisode 24', exact: true }).click();
  await nextRequest;
  assert(episodeRequests.some(item => item.after === 'episode-23' && item.limit === 1));
  assert.deepEqual(errors, []);
  assert.equal(maxEpisodePayload, 24);
  assert(episodeRequests.some(item => item.season === 1 && item.offset === 24));
  assert(episodeRequests.some(item => item.season === 2 && item.offset === 0));
  console.log('10,000 entries: playlist DOM <=50, episode DOM <=24, paging and season reset passed.');
} finally { await browser.close(); }
