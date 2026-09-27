// Drives the web companion in a phone-sized Chromium against a running HydatekOS.
// usage: NODE_PATH=$(npm root -g) node browser-e2e.js <pairing-url> <photo> <out-dir>
const { chromium, devices } = require('playwright');
(async () => {
  const [pairUrl, photo, out] = process.argv.slice(2);
  const url = pairUrl.replace(/^http:\/\/[^/]+/, 'http://' + (process.env.HOST || '127.0.0.1:7743'));
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['Pixel 7'] });
  const page = await ctx.newPage();
  page.on('console', (m) => console.log('[page]', m.text()));
  await page.goto(url);
  await page.waitForFunction(() => document.getElementById('status').textContent.startsWith('Connected'), null, { timeout: 15000 });
  console.log('status:', await page.textContent('#status'));
  console.log('hash cleared:', (await page.evaluate(() => location.hash)) === '');
  await page.screenshot({ path: out + '/web-connected.png' });
  await page.setInputFiles('#photos', photo);
  await page.waitForFunction(() => /Sent 1 item/.test(document.getElementById('sendinfo').textContent), null, { timeout: 20000 });
  console.log('photo:', await page.textContent('#sendinfo'));
  await page.fill('#clip', 'Hello from the browser companion');
  await page.click('#sendclip');
  console.log('clip sent');
  // wait for the PC to send something back (the test harness triggers it)
  await page.waitForFunction(() => document.querySelectorAll('#inbox li:not(.muted)').length >= Number(window.__want || 2), null, { timeout: 90000 });
  const items = await page.$$eval('#inbox li', (lis) => lis.map((l) => l.textContent.trim()));
  console.log('inbox:', JSON.stringify(items));
  await page.screenshot({ path: out + '/web-inbox.png', fullPage: true });
  // reload: pairing survives in localStorage
  await page.reload();
  await page.waitForFunction(() => document.getElementById('status').textContent.startsWith('Connected'), null, { timeout: 15000 });
  console.log('reconnected after reload:', await page.textContent('#status'));
  await browser.close();
})().catch((e) => { console.error('FAIL', e.message); process.exit(1); });
