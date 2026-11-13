import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

const PORT = 5187;
const ORIGIN = `http://127.0.0.1:${PORT}`;
const BASE = `${ORIGIN}/app`;

const SUITES = [
  'scripts/test-setup-ui.mjs',
  'scripts/test-admin-ui.mjs',
  'scripts/test-applications-ui.mjs',
  'scripts/test-artwork-rotation.mjs',
  'scripts/test-i18n-ui.mjs',
  'scripts/test-library-editor-ui.mjs',
  'scripts/test-progressive-pages.mjs',
  'scripts/test-pwa.mjs',
  'scripts/test-tv-ui.mjs',
];

const filter = process.argv.slice(2).join(' ').trim().toLowerCase();
const selected = filter
  ? SUITES.filter(s => s.toLowerCase().includes(filter))
  : SUITES;

if (selected.length === 0) {
  console.error(`No E2E suite matched filter: ${filter}`);
  process.exit(1);
}

async function startPreviewServer() {
  const proc = spawn(
    process.platform === 'win32' ? 'npx.cmd' : 'npx',
    ['vite', 'preview', '--port', String(PORT), '--strictPort', '--host', '127.0.0.1'],
    { stdio: 'ignore' }
  );
  for (let i = 0; i < 60; i++) {
    try {
      const res = await fetch(`${BASE}/`);
      if (res.ok) return proc;
    } catch {
      // Server still starting.
    }
    await delay(500);
  }
  proc.kill('SIGTERM');
  throw new Error(`vite preview did not start on ${BASE}/`);
}

function runSuite(scriptPath) {
  return new Promise((resolve, reject) => {
    console.log(`\n==> Running ${scriptPath}`);
    const child = spawn(process.execPath, [scriptPath], {
      stdio: 'inherit',
      env: {
        ...process.env,
        MEDIA_WEB_TEST_URL: `${BASE}`,
        MEDIA_TEST_ORIGIN: ORIGIN,
        MEDIA_EDITOR_TEST_ORIGIN: ORIGIN,
      },
    });
    child.on('exit', code => {
      if (code === 0) resolve();
      else reject(new Error(`${scriptPath} exited with code ${code}`));
    });
    child.on('error', reject);
  });
}

const server = await startPreviewServer();
try {
  for (const suite of selected) {
    await runSuite(suite);
  }
  console.log(`\nPASS: ${selected.length} Playwright E2E suite(s) succeeded.`);
} finally {
  server.kill('SIGTERM');
}
