const { chromium } = require('../apps/web/node_modules/playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');

(async () => {
  const origin = process.env.MEDIA_TEST_ORIGIN;
  const out = process.env.MEDIA_REPORT_DIR;
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const report = { browser: browser.version(), os: require('node:os').release(), tests: [] };
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const login = await context.request.post(origin + '/api/v1/auth/login', {
      headers: { Origin: origin }, data: { login: 'qa-admin', password: process.env.MEDIA_TEST_ADMIN_PASS }
    });
    assert.equal(login.status(), 200);
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', () => errors.push('pageerror'));
    for (const relay of [false, true]) {
      // Force the actual application fallback without modifying the shipped frontend.
      if (relay) await context.route('**/stream/v1/**', route => route.abort('connectionfailed'));
      for (const kind of ['films', 'series']) {
        await page.goto(origin + '/app/' + kind + '/');
        await page.locator('.media-card').first().waitFor();
        await page.locator('.media-card').first().click();
        const play = kind === 'series'
          ? page.getByRole('button', { name: /^Lire / }).first()
          : page.locator('button').filter({ hasText: /Lecture|Lire/ }).first();
        const playbackStart = performance.now();
        await play.click();
        await page.waitForFunction(() => (document.querySelector('video')?.currentTime ?? 0) > .1, null, { timeout: 30000 });
        const startupMs = performance.now() - playbackStart;
        await page.evaluate(() => { const v = document.querySelector('video'); v.pause(); v.currentTime = 2; });
        await page.waitForFunction(() => { const v = document.querySelector('video'); return v && v.paused && !v.seeking && Math.abs(v.currentTime - 2) < .5; });
        const observation = await page.evaluate(() => {
          const v = document.querySelector('video');
          return { source: v.currentSrc.split('?')[0], currentTime: v.currentTime, width: v.videoWidth, height: v.videoHeight, frames: v.getVideoPlaybackQuality().totalVideoFrames, audioDecodedBytes: v.webkitAudioDecodedByteCount ?? null, paused: v.paused };
        });
        assert.ok(observation.width > 0 && observation.frames > 0);
        if (relay) assert.ok(observation.source.startsWith(origin));
        report.tests.push({ kind, forcedRelay: relay, startupMs, ...observation });
        await page.screenshot({ path: `${out}/${kind}-${relay ? 'relay' : 'default'}.png` });
      }
    }
    assert.deepEqual(errors, []);
    report.success = true;
  } finally {
    fs.writeFileSync(`${out}/browser.json`, JSON.stringify(report, null, 2));
    await browser.close();
  }
})().catch(() => { console.error('P2 browser validation failed; inspect local sanitized report.'); process.exitCode = 1; });
