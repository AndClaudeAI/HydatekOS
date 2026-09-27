// A scripted phone for end-to-end tests: node fake-phone.js <pairing-url> [seconds]
// Connects like the Android app, syncs sample data, then logs every command
// the PC sends (and answers get_photo). UNLOCK=approve|deny makes it answer
// fingerprint unlock requests like a phone whose owner touched the sensor;
// FORGE=1 also sends an approval with the wrong request id first.
const H = require('../hlp.js');
const url = new URL(process.argv[2]);
const secs = Number(process.argv[3] || 60);
const frag = new URLSearchParams(url.hash.slice(1));
const key = H.unb64url(frag.get('k'));
const ws = 'ws://' + (process.env.HOST || url.host) + '/hlp';
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

const thumb = new Uint8Array(64 * 64 * 3);
for (let y = 0; y < 64; y++) for (let x = 0; x < 64; x++) {
  const i = (y * 64 + x) * 3;
  const sky = y < 36;
  thumb[i] = sky ? 228 : 196 - y; thumb[i + 1] = sky ? 183 - y : 137 - y; thumb[i + 2] = sky ? 131 + x : 94;
}

const link = H.connect(ws, key, frag.get('d'), { name: 'Pixel Test', kind: 'android', caps: 'sms,notif,calls,photos,files,clip' + (process.env.UNLOCK ? ',bio' : ''), battery: '64', charging: '1' }, {
  onStatus(t, ok) {
    log('status:', t);
    if (!ok) return;
    const send = (op, f, b) => link.send(op, f, b);
    send('thread', { id: '12', name: 'Ada Lovelace', number: '+15550100' });
    send('msg', { thread: '12', me: '0', time: '09:12', text: 'Did HydatekOS get networking?' });
    send('msg', { thread: '12', me: '1', time: '09:13', text: 'It did. This is coming over TCP from my phone.' });
    send('thread', { id: '7', name: 'Grace Hopper', number: '+15550199' });
    send('msg', { thread: '7', me: '0', time: 'Mon', text: 'Found a bug in the relay.' });
    send('call', { name: 'Ada Lovelace', number: '+15550100', when: 'Today, 08:10', missed: '0' });
    send('call', { name: '', number: '+15550142', when: 'Yesterday, 18:02', missed: '1' });
    send('notif', { id: 'k1', app: 'Calendar', title: 'Stand-up', body: 'In 10 minutes', time: '09:50' });
    send('photo', { id: 'p1', name: 'IMG_0042.jpg', w: '4032', h: '3024', size: '1234' }, thumb);
    if (process.env.BIG) {
      const big = new Uint8Array(Number(process.env.BIG));
      for (let i = 0; i < big.length; i += 65536) require('crypto').randomFillSync(big.subarray(i, i + 65536));
      const t0 = Date.now();
      send('file', { name: 'big.bin', size: String(big.length) }, big);
      log('sent big.bin', big.length, 'bytes sha256=' + H.hex(H.sha256(big)).slice(0, 16));
      const wait = setInterval(() => { if (link.buffered === 0) { clearInterval(wait); log('big.bin drained after', Date.now() - t0, 'ms'); } }, 20);
    }
    setTimeout(() => {
      send('msg', { thread: '12', me: '0', time: '09:20', text: 'Reply to me from the PC!', live: '1' });
      send('notif', { id: 'k2', app: 'Weather', title: 'Rain soon', body: 'Showers from 11:00', time: '09:21', live: '1' });
      send('clip', { text: 'Copied on the phone' });
      send('file', { name: 'boarding-pass.txt', size: '24' }, new TextEncoder().encode('Gate B12 - Seat 14A\n'));
      send('battery', { level: '63', charging: '0' });
    }, 1500);
  },
  onMessage(m) {
    log('from PC:', m.op, JSON.stringify(m.fields), m.blob.length ? `+${m.blob.length} bytes sha256=${H.hex(H.sha256(m.blob)).slice(0, 16)}` : '');
    if (m.op === 'unlock_req' && process.env.FORGE) {
      // an approval for some other request must be ignored
      link.send('unlock', { id: '0123456789abcdef', ok: '1' });
    }
    if (m.op === 'unlock_req' && process.env.UNLOCK) {
      setTimeout(() => link.send('unlock', { id: m.fields.id, ok: process.env.UNLOCK === 'approve' ? '1' : '0' }), 2500);
    }
    if (m.op === 'get_photo') link.send('file', { name: 'IMG_0042.jpg', size: '10' }, new Uint8Array([0xff, 0xd8, 1, 2, 3, 4, 5, 6, 0xff, 0xd9]));
  },
  onFatal(r) { log('fatal:', r); process.exit(2); },
  onClose() { log('closed'); },
});
setTimeout(() => { log('done'); link.close(); process.exit(0); }, secs * 1000);
