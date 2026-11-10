import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';

// TV layout driven by the remote only (no mouse): home → card → work page → playback → media keys → Back,
// against a synthetic Core (API mocked with page.route). The Tizen and webOS keys (Back 10009 / 461) are sent
// through CDP so the page receives their legacy key codes like on a TV. Run after `npm run build`; it starts
// `vite preview` itself unless MEDIA_WEB_TEST_URL points at a running server.
const external = process.env.MEDIA_WEB_TEST_URL;
const base = external ?? 'http://127.0.0.1:5189/app';
const launch = process.env.MEDIA_WEB_TEST_CHANNEL ? { channel: process.env.MEDIA_WEB_TEST_CHANNEL } : {};
const report = process.env.MEDIA_TV_REPORT_DIR;

/** Three seconds of a quiet tone: a real, finite media file the browser plays without codecs. */
function toneWav(seconds = 3, rate = 8000) {
  const samples = seconds * rate;
  const data = Buffer.alloc(44 + samples * 2);
  data.write('RIFF', 0); data.writeUInt32LE(36 + samples * 2, 4); data.write('WAVE', 8);
  data.write('fmt ', 12); data.writeUInt32LE(16, 16); data.writeUInt16LE(1, 20); data.writeUInt16LE(1, 22);
  data.writeUInt32LE(rate, 24); data.writeUInt32LE(rate * 2, 28); data.writeUInt16LE(2, 32); data.writeUInt16LE(16, 34);
  data.write('data', 36); data.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i += 1) data.writeInt16LE(Math.round(Math.sin((i / rate) * 2 * Math.PI * 440) * 800), 44 + i * 2);
  return data;
}
const wav = toneWav(20);

const works = ['Alpha', 'Bravo', 'Charlie'].map((title, index) => ({
  id: `w${index + 1}`, kind: 'film', title, year: 2000 + index, description: `${title}, un film de test.`, libraryId: 'lib', synthetic: false,
  posterUrl: null, available: true,
  editions: [{ id: `e${index + 1}`, label: 'Standard', versions: [{ id: `v${index + 1}`, label: 'WAV', sources: [{ playbackMediaId: `m${index + 1}` }] }] }]
}));
const media = id => ({
  id, workId: `w${id.slice(1)}`, title: works[Number(id.slice(1)) - 1].title, available: true, subtitles: [],
  sources: [{ id: `${id}-fr`, audioTrackId: `${id}-fr`, language: 'fr', url: `/media-fixture/${id}.wav`, mime: 'audio/wav', durationSeconds: 3 }]
});

async function preview() {
  if (external) return null;
  const server = spawn('npx', ['vite', 'preview', '--port', '5189', '--strictPort', '--host', '127.0.0.1'], { stdio: 'ignore' });
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try { if ((await fetch(`${base}/`)).ok) return server; } catch { /* not listening yet */ }
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  server.kill();
  throw new Error('vite preview did not start');
}

const server = await preview();
const browser = await chromium.launch({ headless: true, ...launch });
try {
  const context = await browser.newContext({ locale: 'fr-FR', viewport: { width: 1280, height: 720 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const hellos = [];
  await page.routeWebSocket('**/api/v1/remote/ws', socket => {
    socket.onMessage(message => {
      const parsed = JSON.parse(String(message));
      if (parsed.type === 'hello') { hellos.push(parsed); socket.send(JSON.stringify({ type: 'devices', devices: [] })); }
    });
  });
  // Byte ranges, as the Core serves media: the browser seeks with them.
  await page.route('**/media-fixture/*.wav', route => {
    const range = /bytes=(\d+)-(\d*)/.exec(route.request().headers().range ?? '');
    if (!range) return route.fulfill({ status: 200, contentType: 'audio/wav', headers: { 'accept-ranges': 'bytes' }, body: wav });
    const start = Number(range[1]);
    const end = range[2] ? Math.min(Number(range[2]), wav.length - 1) : wav.length - 1;
    return route.fulfill({ status: 206, contentType: 'audio/wav', headers: { 'accept-ranges': 'bytes', 'content-range': `bytes ${start}-${end}/${wav.length}` }, body: wav.subarray(start, end + 1) });
  });
  await page.route('**/api/v1/**', async route => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const detail = path.match(/\/catalog\/(w\d)$/);
    const descriptor = path.match(/\/media\/(m\d)$/);
    if (path.endsWith('/auth/me')) return route.fulfill({ json: { user: { id: 'ana', login: 'ana', displayName: 'Ana', role: 'user' }, csrfToken: 'csrf' } });
    if (path.endsWith('/auth/status')) return route.fulfill({ json: { initialized: true } });
    if (path.endsWith('/libraries')) return route.fulfill({ json: { libraries: [{ id: 'lib', name: 'Films', kind: 'films' }] } });
    if (path.endsWith('/preferences')) return request.method() === 'PUT' ? route.fulfill({ status: 204 }) : route.fulfill({ json: { locale: 'fr' } });
    if (path.endsWith('/catalog')) return route.fulfill({ json: { items: works, total: works.length, hasMore: false, offset: 0, limit: 24, notice: '' } });
    if (detail) return route.fulfill({ json: works.find(work => work.id === detail[1]) });
    if (descriptor) return route.fulfill({ json: media(descriptor[1]) });
    if (path.includes('/account/playback/')) return route.fulfill({ status: 204 });
    if (/\/progress\/./.test(path)) return request.method() === 'GET' ? route.fulfill({ status: 404, json: { error: 'not_found' } }) : route.fulfill({ status: 204 });
    if (path.endsWith('/progress')) return route.fulfill({ json: { items: [], total: 0 } });
    if (path.endsWith('/server')) return route.fulfill({ json: { name: 'Synthetic' } });
    return route.fulfill({ status: 404, json: { error: 'not_found' } });
  });

  const cdp = await context.newCDPSession(page);
  /** A key as a TV remote sends it: `Unidentified` with the platform's key code. */
  async function tvRemote(keyCode) {
    for (const type of ['rawKeyDown', 'keyUp']) await cdp.send('Input.dispatchKeyEvent', { type, key: 'Unidentified', windowsVirtualKeyCode: keyCode, nativeVirtualKeyCode: keyCode });
  }
  const focused = () => page.evaluate(() => {
    const element = document.activeElement;
    return element && element !== document.body ? { tag: element.tagName, label: element.getAttribute('aria-label') ?? element.textContent?.trim().slice(0, 40), cls: element.className } : null;
  });
  async function press(key) { await page.keyboard.press(key); await page.waitForTimeout(80); return focused(); }

  await page.goto(`${base}/?tv=1`);
  assert.equal(await page.locator('html').getAttribute('data-tv'), 'true', '?tv=1 turns the TV layout on');
  await page.locator('a.media-card').first().waitFor();

  const label = async () => (await focused())?.label;
  const expectFocus = async (key, expected, message) => {
    await press(key);
    assert.equal(await label(), expected, message ?? `${key} focuses ${expected}`);
  };

  // Home: the first arrow lands on the hero, then rails go card by card and stop at their end.
  await expectFocus('ArrowDown', 'Lecture', 'the first arrow focuses the featured work');
  await expectFocus('ArrowDown', 'Voir Alpha', 'down reaches the first card of the rail');
  await expectFocus('ArrowRight', 'Voir Bravo');
  await expectFocus('ArrowRight', 'Voir Charlie');
  await expectFocus('ArrowRight', 'Voir Charlie', 'the end of a rail keeps the focus');
  await expectFocus('ArrowLeft', 'Voir Bravo');
  await expectFocus('ArrowUp', 'Plus d’infos', 'up returns to the aligned hero button, not to the sticky bar');
  await expectFocus('ArrowLeft', 'Lecture');
  await press('ArrowUp');
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('header'))), 'the top bar comes once the page has nothing above');
  assert.equal(await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle), 'solid', 'the focus is visible');
  await press('ArrowDown');
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('main'))), 'down from the bar returns to the page');
  await expectFocus('ArrowDown', 'Voir Alpha');
  await expectFocus('ArrowRight', 'Voir Bravo');
  if (report) { await page.waitForTimeout(500); await page.screenshot({ path: `${report}/tv-home.png` }); }

  // OK opens the work page; down reaches its play button; OK starts playback.
  await page.keyboard.press('Enter');
  await page.waitForURL(/work=w2/);
  await page.getByRole('button', { name: 'Lancer le film' }).waitFor();
  await expectFocus('ArrowDown', 'Lancer le film');
  if (report) { await page.waitForTimeout(500); await page.screenshot({ path: `${report}/tv-detail.png` }); }
  await page.keyboard.press('Enter');
  await page.locator('dialog#player[open]').waitFor();
  await page.waitForFunction(() => { const video = document.querySelector('dialog#player video'); return video && !video.paused && video.currentTime > 0.3; }, null, { timeout: 15000 });
  const paused = () => page.evaluate(() => document.querySelector('dialog#player video').paused);

  // Media keys: the keyboard's play/pause, then Tizen and webOS codes (415 play, 19 pause, 417 forward).
  await page.keyboard.press('MediaPlayPause');
  await page.waitForFunction(() => document.querySelector('dialog#player video').paused);
  await tvRemote(415);
  await page.waitForFunction(() => !document.querySelector('dialog#player video').paused);
  await tvRemote(19);
  await page.waitForFunction(() => document.querySelector('dialog#player video').paused);
  const before = await page.evaluate(() => document.querySelector('dialog#player video').currentTime);
  await tvRemote(417);
  await page.waitForFunction(start => document.querySelector('dialog#player video').currentTime >= start + 5, before);
  // OK on the picture plays again; up shows the controls with the focus on play, without touching the volume.
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => !document.querySelector('dialog#player video').paused);
  const volume = await page.evaluate(() => document.querySelector('dialog#player video').volume);
  await page.keyboard.press('ArrowUp');
  await page.waitForFunction(() => document.activeElement?.classList.contains('play-btn'));
  assert.equal(await page.evaluate(() => document.querySelector('dialog#player video').volume), volume, 'up does not change the volume on a TV');
  await press('ArrowRight');
  assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('.player-controls')) && !document.activeElement.classList.contains('play-btn')), 'arrows move between the player controls');
  assert.equal(await paused(), false, 'moving between controls does not pause');
  if (report) await page.screenshot({ path: `${report}/tv-player.png` });

  // Back (Tizen 10009) closes the player and gives the focus back; Back (webOS 461) returns home.
  await tvRemote(10009);
  await page.waitForFunction(() => !document.querySelector('dialog#player').open);
  assert.match(page.url(), /work=w2/, 'Back first leaves the player only');
  // The player gives the focus back once it has left full screen.
  await page.waitForFunction(() => document.activeElement?.classList.contains('hero-cta'), null, { timeout: 3000 }).catch(() => {});
  assert.equal(await label(), 'Lancer le film', 'the focus returns to the play button');
  await tvRemote(461);
  await page.waitForURL(url => !url.search.includes('work='));
  await page.locator('a.media-card').first().waitFor();

  assert.equal(hellos.length > 0, true, 'the TV joins the remote-control channel');
  assert.deepEqual([...new Set(hellos.map(hello => hello.kind))], ['tv'], 'and announces itself as a TV');
  assert.deepEqual(errors, []);
  console.log('TV journey: home, rail, work page, playback, media keys and Back with the keyboard only — ok');
} finally {
  await browser.close();
  server?.kill();
}
