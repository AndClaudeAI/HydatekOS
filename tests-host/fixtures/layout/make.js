// Lays out cases.json in Chromium and records each box (id -> x, y, w, h).
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const fs = require('fs');
const palette = ['#e6194b', '#3cb44b', '#4363d8', '#f58231', '#911eb4', '#46f0f0', '#f032e6', '#bcf60c', '#008080', '#9a6324'];
(async () => {
  const cases = JSON.parse(fs.readFileSync(__dirname + '/cases.json'));
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1000, height: 800 } });
  const out = [];
  for (const c of cases) {
    for (const v of c.variants) {
      const ids = [...c.html.matchAll(/id=(\w+)/g)].map(m => m[1]);
      const colours = ids.map((id, i) => `#${id}{background:${palette[i % palette.length]}}`).join('');
      const html = `<!doctype html><html><head><style>*{box-sizing:border-box}body{margin:0}.c,.g{${v}} ${c.css} ${colours}</style></head><body>${c.html}</body></html>`;
      await page.setContent(html);
      const boxes = await page.evaluate(ids => ids.map(id => { const r = document.getElementById(id).getBoundingClientRect(); return [id, Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)]; }), ids);
      out.push({ name: c.name + (v ? ' / ' + v : ''), html, boxes, colours: ids.map((id, i) => [id, palette[i % palette.length]]) });
    }
  }
  // one case per block: "case <name>", the page on one line, then "<id> <#colour> x y w h"
  const txt = out.map(o => ['case ' + o.name, o.html, ...o.boxes.map((b, i) => [b[0], o.colours[i][1], b[1], b[2], b[3], b[4]].join(' '))].join('\n')).join('\n\n');
  fs.writeFileSync(__dirname + '/expected.txt', txt + '\n');
  await browser.close();
  console.log(out.length + ' layouts');
})();
