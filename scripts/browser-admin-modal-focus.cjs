// Browser check for the shared administration modal action; no Core required.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('../apps/web/node_modules/typescript');
const { chromium } = require('../apps/web/node_modules/playwright');

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent('<button id="open">Open</button><div id="backdrop"><div role="dialog" aria-modal="true"><button id="first">Close</button><input id="field"><button id="last">Save</button></div></div><button id="outside">Outside</button>');
    const source = fs.readFileSync('apps/web/src/lib/admin/modal-focus.ts', 'utf8');
    const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
    await page.addScriptTag({ content: `const exports={};${js};window.modalFocus=exports.modalFocus;` });
    await page.locator('#open').focus();
    await page.evaluate(() => { window.modalClosedCount = 0; window.action = window.modalFocus(document.querySelector('#backdrop'), () => { window.modalClosedCount++; window.action.destroy(); }); });
    assert.equal(await page.evaluate(() => document.activeElement.id), 'first');
    await page.keyboard.press('Shift+Tab');
    assert.equal(await page.evaluate(() => document.activeElement.id), 'last');
    await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(() => document.activeElement.id), 'first');
    await page.locator('#outside').focus();
    assert.equal(await page.evaluate(() => document.activeElement.id), 'first');
    await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(() => window.modalClosedCount), 1);
    assert.equal(await page.evaluate(() => document.activeElement.id), 'open');
    await page.evaluate(() => { window.action = window.modalFocus(document.querySelector('#backdrop'), () => { window.modalClosedCount++; window.action.destroy(); }); document.querySelector('#backdrop').click(); });
    assert.equal(await page.evaluate(() => window.modalClosedCount), 2);
    assert.equal(await page.evaluate(() => document.activeElement.id), 'open');
    console.log('Administration modal keyboard/focus checks passed');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
