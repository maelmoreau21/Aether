// GitHub-hosted images already include Chrome. Verify it before paying for an
// apt refresh and reinstall; fall back to the existing official installer.
const {chromium} = require('../apps/web/node_modules/playwright');
const {execFileSync} = require('node:child_process');
async function verify() {
  const browser = await chromium.launch({channel:'chrome',headless:true,timeout:30000});
  try {
    const page = await browser.newPage();
    await page.goto('data:text/html,<title>CI Chrome check</title>');
    if(await page.title() !== 'CI Chrome check') throw new Error('Chrome smoke check failed');
    console.log(`Chrome ready: ${browser.version()}`);
  } finally { await browser.close(); }
}
(async()=>{
  try { await verify(); }
  catch(error) {
    console.log(`Preinstalled Chrome unavailable (${error.name}); installing Chrome and dependencies.`);
    execFileSync(process.execPath,[require.resolve('../apps/web/node_modules/playwright/cli.js'),
      'install','--with-deps','chrome'],{stdio:'inherit'});
    await verify();
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
