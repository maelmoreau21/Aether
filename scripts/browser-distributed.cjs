const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');

const origin = process.env.MEDIA_TEST_ORIGIN || 'http://127.0.0.1:3000';
const node1Url = process.env.MEDIA_TEST_NODE1_URL || 'http://aether-node-films:4101';
const node1PublicUrl = process.env.MEDIA_TEST_NODE1_PUBLIC_URL || 'http://127.0.0.1:4101';
const node1Token = process.env.MEDIA_TEST_NODE1_TOKEN;
const node2Url = process.env.MEDIA_TEST_NODE2_URL || 'http://aether-node-series:4102';
const node2PublicUrl = process.env.MEDIA_TEST_NODE2_PUBLIC_URL || 'http://127.0.0.1:4102';
const node2Token = process.env.MEDIA_TEST_NODE2_TOKEN;
const adminLogin = process.env.MEDIA_TEST_ADMIN_LOGIN || 'admin-synology';
const adminPass = process.env.MEDIA_TEST_ADMIN_PASS || 'distributed-docker-pass-2026';

const out = process.env.MEDIA_REPORT_DIR || 'docs/evidence/2026-10-17/distributed';
fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = {
    browser: browser.version(),
    os: require('node:os').release(),
    headless: true,
    origin,
    node1Url,
    node1PublicUrl,
    node2Url,
    node2PublicUrl,
    directStreamResponses: [],
    results: {}
  };

  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const page = await context.newPage();
    const errors = [];

    page.on('response', async response => {
      const url = response.url();
      if (url.includes('/stream/v1/assets/')) {
        report.directStreamResponses.push({
          url: url.split('?')[0],
          status: response.status(),
          acceptRanges: response.headers()['accept-ranges'] || null,
          contentRange: response.headers()['content-range'] || null
        });
      }
    });
    page.on('pageerror', error => errors.push(String(error)));
    page.on('console', message => { if (message.type() === 'error') errors.push('browser console error'); });
    page.on('response', res => {
      if (res.url().includes('/api/v1/')) {
        console.log('API RESPONSE:', res.status(), res.url());
      }
    });

    // 1. Log in as administrator
    await page.goto(origin + '/app/');
    await page.waitForURL('**/app/login/**');
    await page.locator('input[autocomplete="username"]').fill(adminLogin);
    await page.locator('input[autocomplete="current-password"]').fill(adminPass);
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(origin + '/app/');
    report.results.login = true;

    // 2. Navigate to Admin Libraries & Nodes page and pair both remote Nodes via UI
    await page.goto(origin + '/app/admin/libraries/');
    await page.locator('#pair-node-name').waitFor();

    const nodeListSection = page.locator('section[aria-labelledby="node-list"]');

    // Pair Node 1 (Films) if not present
    if (await nodeListSection.getByText('Node Films Docker').count() === 0) {
      await page.locator('#pair-node-name').fill('Node Films Docker');
      await page.locator('#pair-node-base-url').fill(node1Url);
      await page.locator('#pair-node-public-url').fill(node1PublicUrl);
      await page.locator('#pair-node-token').fill(node1Token);
      await page.getByRole('button', { name: 'Appairer le Node', exact: true }).click();
      await nodeListSection.getByText('Node Films Docker').first().waitFor({ timeout: 15000 });
    }
    report.results.pairNode1 = true;

    // Pair Node 2 (Series) if not present
    if (await nodeListSection.getByText('Node Séries Docker').count() === 0) {
      await page.locator('#pair-node-name').fill('Node Séries Docker');
      await page.locator('#pair-node-base-url').fill(node2Url);
      await page.locator('#pair-node-public-url').fill(node2PublicUrl);
      await page.locator('#pair-node-token').fill(node2Token);
      await page.getByRole('button', { name: 'Appairer le Node', exact: true }).click();
      await nodeListSection.getByText('Node Séries Docker').first().waitFor({ timeout: 15000 });
    }
    report.results.pairNode2 = true;
    await page.screenshot({ path: `${out}/01-nodes-paired.png`, fullPage: true });

    // 3. Create Films library on Node 1 and Series library on Node 2 via UI if not present
    await page.locator('.admin-sidebar a[href$="#libraries-section"]').click();
    const libListSection = page.locator('#libraries-section');
    const nodeSelect = page.locator('#library-node');

    if (await libListSection.getByText('Films NAS Node 1').count() === 0) {
      await page.getByRole('button',{name:'Ajouter une bibliothèque',exact:true}).click();
      const node1OptionValue = await nodeSelect.locator('option', { hasText: 'Node Films Docker' }).getAttribute('value');
      assert.ok(node1OptionValue, 'Node 1 option must exist in #library-node select');
      await page.locator('#library-name').fill('Films NAS Node 1');
      await page.locator('#library-kind').selectOption('films');
      await nodeSelect.selectOption(node1OptionValue);
      await page.locator('#library-root').selectOption('/media/films');
      await page.getByRole('button', {name:'Ajouter ce dossier', exact:true}).click();
      await page.getByRole('button', { name: 'Enregistrer', exact: true }).click();
      await page.getByRole('heading', { name: 'Films NAS Node 1', exact: true }).waitFor({ timeout: 15000 });
    }

    if (await libListSection.getByText('Séries NAS Node 2').count() === 0) {
      await page.getByRole('button',{name:'Ajouter une bibliothèque',exact:true}).click();
      const node2OptionValue = await nodeSelect.locator('option', { hasText: 'Node Séries Docker' }).getAttribute('value');
      assert.ok(node2OptionValue, 'Node 2 option must exist in #library-node select');
      await page.locator('#library-name').fill('Séries NAS Node 2');
      await page.locator('#library-kind').selectOption('series');
      await nodeSelect.selectOption(node2OptionValue);
      await page.locator('#library-root').selectOption('/media/series');
      await page.getByRole('button', {name:'Ajouter ce dossier', exact:true}).click();
      await page.getByRole('button', { name: 'Enregistrer', exact: true }).click();
      await page.getByRole('heading', { name: 'Séries NAS Node 2', exact: true }).waitFor({ timeout: 15000 });
    }
    report.results.librariesCreated = true;

    // 4. Trigger scans on both remote Nodes from UI and wait for completion
    await page.getByLabel('Actions Films NAS Node 1',{exact:true}).click();
    await page.getByRole('button',{name:'Actualiser la bibliothèque',exact:true}).click();
    await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
    const filmLibArticle = page.locator('article').filter({ has: page.getByRole('heading', { name: 'Films NAS Node 1', exact: true }) });
    await filmLibArticle.getByText(/Terminé/).waitFor({ timeout: 60000 });

    await page.getByLabel('Actions Séries NAS Node 2',{exact:true}).click();
    await page.getByRole('button',{name:'Actualiser la bibliothèque',exact:true}).click();
    await page.getByRole('dialog').getByRole('button',{name:'Actualiser',exact:true}).click();
    const seriesLibArticle = page.locator('article').filter({ has: page.getByRole('heading', { name: 'Séries NAS Node 2', exact: true }) });
    await seriesLibArticle.getByText(/Terminé/).waitFor({ timeout: 60000 });
    report.results.remoteScansCompleted = true;
    await page.screenshot({ path: `${out}/02-remote-scans-complete.png`, fullPage: true });

    // 5. Browse Films catalogue, open film on Node 1, play, pause, and seek
    const tCatStart = performance.now();
    await page.goto(origin + '/app/films/');
    const filmCard = page.locator('.media-card').first();
    await filmCard.waitFor({ timeout: 15000 });
    report.results.catalogLatencyMs = Math.round(performance.now() - tCatStart);
    await page.screenshot({ path: `${out}/03-catalog-films.png` });

    await filmCard.click();
    const playFilmBtn = page.locator('button.primary-play, button').filter({ hasText: /Lecture|Lire/ }).first();
    await playFilmBtn.waitFor({ timeout: 15000 });
    await playFilmBtn.click();

    await page.waitForFunction(() => document.querySelector('video') !== null, null, { timeout: 15000 });
    await page.waitForFunction(() => (document.querySelector('video')?.currentTime ?? 0) > 0.05, null, { timeout: 15000 });
    const filmVideoSrc = await page.evaluate(() => document.querySelector('video')?.currentSrc || document.querySelector('video')?.src);
    report.results.filmVideoSrc = filmVideoSrc.split('?')[0];
    assert.ok(filmVideoSrc.startsWith(node1PublicUrl), `Expected direct stream from Node 1 (${node1PublicUrl}), got ${filmVideoSrc}`);

    // Pause & seek on Node 1 film
    await page.evaluate(() => {
      const v = document.querySelector('video');
      v.pause();
      v.currentTime = 1.5;
    });
    await page.waitForFunction(() => {
      const v = document.querySelector('video');
      return v && v.paused && Math.abs(v.currentTime - 1.5) < 0.6;
    }, null, { timeout: 15000 });
    report.results.filmPlaybackAndSeek = true;
    await page.screenshot({ path: `${out}/04-film-direct-playback.png` });

    // 6. Browse Series catalogue, open series on Node 2, play episode, pause, and seek
    await page.goto(origin + '/app/series/');
    const seriesCard = page.locator('.media-card').first();
    await seriesCard.waitFor({ timeout: 15000 });
    await seriesCard.click();

    const playEpBtn = page.getByRole('button', { name: /^Lire / }).first();
    await playEpBtn.waitFor({ timeout: 15000 });
    await playEpBtn.click();

    await page.waitForFunction(() => document.querySelector('video') !== null, null, { timeout: 15000 });
    await page.waitForFunction(() => (document.querySelector('video')?.currentTime ?? 0) > 0.05, null, { timeout: 15000 });
    const epVideoSrc = await page.evaluate(() => document.querySelector('video')?.currentSrc || document.querySelector('video')?.src);
    report.results.episodeVideoSrc = epVideoSrc.split('?')[0];
    assert.ok(epVideoSrc.startsWith(node2PublicUrl), `Expected direct stream from Node 2 (${node2PublicUrl}), got ${epVideoSrc}`);

    await page.evaluate(() => {
      const v = document.querySelector('video');
      v.pause();
      v.currentTime = 1.2;
    });
    await page.waitForFunction(() => {
      const v = document.querySelector('video');
      return v && v.paused && Math.abs(v.currentTime - 1.2) < 0.6;
    }, null, { timeout: 15000 });
    report.results.episodePlaybackAndSeek = true;
    await page.screenshot({ path: `${out}/05-episode-direct-playback.png` });

    assert.equal(errors.length, 0, `Unexpected page/CSP errors: ${errors.join('; ')}`);
    assert.ok(
      report.directStreamResponses.some(r => r.status === 206),
      'Expected at least one HTTP 206 Partial Content response from direct Node streaming'
    );

    fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2));
    console.log('DISTRIBUTED E2E SUCCESS:', JSON.stringify(report.results));
  } finally {
    await browser.close();
  }
})();
