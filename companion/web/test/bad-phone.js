// Negative tests: wrong key and wrong pairing id must be rejected.
const H = require('../hlp.js');
const url = new URL(process.argv[2]);
const frag = new URLSearchParams(url.hash.slice(1));
const ws = 'ws://' + (process.env.HOST || url.host) + '/hlp';
function attempt(label, key, pair) {
  return new Promise((done) => {
    let got = false;
    const l = H.connect(ws, key, pair, { name: 'Intruder', kind: 'web', caps: '' }, {
      onStatus(t, ok) { if (ok) setTimeout(() => l.send('clip', { text: 'should not arrive' }), 100); },
      onMessage(m) { got = true; console.log(label, 'UNEXPECTED message', m.op); },
      onFatal(r) { console.log(label, 'rejected:', r); },
      onClose() { console.log(label, got ? 'FAIL (was served)' : 'closed without service: OK'); done(); },
    });
    setTimeout(() => { l.close(); }, 3000);
  });
}
(async () => {
  const good = H.unb64url(frag.get('k'));
  const bad = good.slice(); bad[0] ^= 1;
  await attempt('[wrong key]', bad, frag.get('d'));
  await attempt('[wrong pair id]', good, 'ffffff');
})();
