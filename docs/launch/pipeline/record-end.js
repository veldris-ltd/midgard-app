const { chromium } = require('playwright-core');
// Edge by default; CHROME_PATH names any other Chromium build.
const BROWSER = process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'msedge' };
const fs = require('fs'), path = require('path');
(async () => {
  const b = await chromium.launch({ ...BROWSER, headless: true, args: ['--host-resolver-rules=MAP localhost 127.0.0.1, MAP * ~NOTFOUND, EXCLUDE 127.0.0.1', '--hide-scrollbars'] });
  const ctx = await b.newContext({ viewport: { width: 1920, height: 1080 }, recordVideo: { dir: 'out/raw-end', size: { width: 1920, height: 1080 } } });
  const p = await ctx.newPage();
  await p.goto('http://localhost:5173/launch/end.html'); await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(6500);
  const v = p.video(); await ctx.close();
  fs.copyFileSync(await v.path(), 'out/end.webm'); await b.close(); console.log('ok');
})();
