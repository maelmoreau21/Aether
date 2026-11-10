// Checks the TV packaging files without the vendor SDKs: config.xml is well-formed XML (Chromium's parser)
// with the identifiers, privileges and files Tizen requires; appinfo.json carries webOS's mandatory fields;
// icons have the launcher sizes; the shell normalises Core addresses. `node apps/tv/scripts/validate.mjs`
// after `npm ci` in apps/web.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const require = createRequire(new URL('../../web/package.json', import.meta.url));
const here = path => fileURLToPath(new URL(path, import.meta.url));
const read = path => readFileSync(here(path), 'utf8');

function pngSize(path) {
  const data = readFileSync(here(path));
  assert.equal(data.subarray(1, 4).toString('latin1'), 'PNG', `${path} is a PNG`);
  return { width: data.readUInt32BE(16), height: data.readUInt32BE(20) };
}

// Tizen config.xml
const config = read('../tizen/config.xml');
const { chromium } = require('playwright');
const browser = await chromium.launch({ headless: true });
let widget;
try {
  const page = await browser.newPage();
  widget = await page.evaluate(xml => {
    const doc = new DOMParser().parseFromString(xml, 'application/xml');
    const error = doc.querySelector('parsererror');
    if (error) return { error: error.textContent };
    const TIZEN = 'http://tizen.org/ns/widgets';
    const root = doc.documentElement;
    const one = (ns, name) => root.getElementsByTagNameNS(ns, name)[0];
    const W3C = 'http://www.w3.org/ns/widgets';
    const app = one(TIZEN, 'application');
    return {
      root: root.localName, ns: root.namespaceURI, id: root.getAttribute('id'), version: root.getAttribute('version'),
      appId: app?.getAttribute('id'), package: app?.getAttribute('package'), required: app?.getAttribute('required_version'),
      content: one(W3C, 'content')?.getAttribute('src'), icon: one(W3C, 'icon')?.getAttribute('src'), name: one(W3C, 'name')?.textContent,
      privileges: [...root.getElementsByTagNameNS(TIZEN, 'privilege')].map(node => node.getAttribute('name')),
      profile: one(TIZEN, 'profile')?.getAttribute('name'),
      navigation: one(TIZEN, 'allow-navigation')?.textContent, access: one(W3C, 'access')?.getAttribute('origin')
    };
  }, config);
} finally {
  await browser.close();
}
assert.equal(widget.error, undefined, `config.xml: ${widget.error}`);
assert.equal(widget.root, 'widget');
assert.equal(widget.ns, 'http://www.w3.org/ns/widgets');
assert.ok(widget.id, 'widget id');
assert.match(widget.version, /^\d+\.\d+\.\d+$/, 'Tizen version is x.y.z');
assert.match(widget.package, /^[0-9a-zA-Z]{10}$/, 'Tizen package id: 10 alphanumeric characters');
assert.match(widget.appId, new RegExp(`^${widget.package}\\.[0-9a-zA-Z]{1,52}$`), 'application id is <package>.<name>');
assert.match(widget.required, /^\d+\.\d+$/);
assert.equal(widget.name, 'Aether');
assert.equal(widget.profile, 'tv-samsung');
assert.ok(widget.privileges.includes('http://tizen.org/privilege/internet'), 'internet privilege');
assert.ok(widget.privileges.includes('http://tizen.org/privilege/tv.inputdevice'), 'media keys privilege');
assert.equal(widget.navigation, '*', 'the shell navigates to the Core');
assert.equal(widget.access, '*');
assert.equal(widget.content, 'index.html');
assert.ok(existsSync(here(`../shell/${widget.content}`)), 'Tizen content in shell/');
assert.deepEqual(pngSize(`../tizen/${widget.icon}`), { width: 512, height: 423 }, 'Tizen TV icon 512 × 423');

// webOS appinfo.json
const appinfo = JSON.parse(read('../webos/appinfo.json'));
assert.match(appinfo.id, /^[a-z0-9][a-z0-9.-]*[a-z0-9]$/, 'webOS id: lowercase letters, digits, dots and dashes');
assert.ok(!/^(com\.(webos|palm|lge)|com\.lg)\./.test(appinfo.id), 'webOS id outside reserved prefixes');
assert.match(appinfo.version, /^\d+\.\d+\.\d+$/, 'webOS version is x.y.z');
assert.equal(appinfo.version, widget.version, 'both TVs ship the same version');
assert.equal(appinfo.type, 'web');
for (const field of ['vendor', 'title', 'main', 'icon']) assert.ok(appinfo[field], `appinfo.${field}`);
assert.ok(existsSync(here(`../shell/${appinfo.main}`)), 'webOS main in shell/');
assert.equal(appinfo.disableBackHistoryAPI, true, 'Back reaches the page');
assert.deepEqual(pngSize(`../webos/${appinfo.icon}`), { width: 80, height: 80 }, 'webOS icon 80 × 80');
assert.deepEqual(pngSize(`../webos/${appinfo.largeIcon}`), { width: 130, height: 130 }, 'webOS large icon 130 × 130');

// Shell
const html = read('../shell/index.html');
for (const asset of ['shell.css', 'shell.js', 'logo.svg']) {
  assert.ok(html.includes(asset), `index.html loads ${asset}`);
  assert.ok(existsSync(here(`../shell/${asset}`)), `shell/${asset}`);
}
const shell = require(here('../shell/shell.js'));
assert.equal(shell.normalizeAddress('aether.maison.lan'), 'https://aether.maison.lan');
assert.equal(shell.normalizeAddress(' https://aether.maison.lan:8443/app/ '), 'https://aether.maison.lan:8443');
assert.equal(shell.normalizeAddress('http://127.0.0.1:3000'), 'http://127.0.0.1:3000');
assert.equal(shell.normalizeAddress('ftp://aether.lan'), null);
assert.equal(shell.normalizeAddress('https://user:secret@aether.lan'), null);
assert.equal(shell.normalizeAddress(''), null);
assert.equal(shell.remoteKey({ key: 'Unidentified', keyCode: 10009 }), 'back');
assert.equal(shell.remoteKey({ key: 'GoBack', keyCode: 461 }), 'back');

console.log('TV packaging files: valid');
