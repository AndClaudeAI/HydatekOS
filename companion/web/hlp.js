/* Hydatek Link Protocol (HLP/1) client for browsers.
 *
 * Pages served over plain http:// on a home network cannot use WebCrypto
 * (crypto.subtle needs a secure context), so SHA-256, HKDF and
 * ChaCha20-Poly1305 are implemented here. crypto.getRandomValues is still
 * available and provides the nonces. See docs/PHONE_LINK.md.
 */
(function (root) {
  'use strict';
  const te = new TextEncoder();
  const td = new TextDecoder();

  // ---------------------------------------------------------------- SHA-256
  const K = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2]);

  function sha256(data) {
    const len = data.length;
    const padLen = ((len + 9 + 63) >> 6) << 6;
    const m = new Uint8Array(padLen);
    m.set(data);
    m[len] = 0x80;
    const dv = new DataView(m.buffer);
    dv.setUint32(padLen - 8, Math.floor(len / 0x20000000));
    dv.setUint32(padLen - 4, (len << 3) >>> 0);
    const h = new Uint32Array([0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]);
    const w = new Uint32Array(64);
    for (let o = 0; o < padLen; o += 64) {
      for (let i = 0; i < 16; i++) w[i] = dv.getUint32(o + 4 * i);
      for (let i = 16; i < 64; i++) {
        const a = w[i - 15], b = w[i - 2];
        const s0 = ((a >>> 7) | (a << 25)) ^ ((a >>> 18) | (a << 14)) ^ (a >>> 3);
        const s1 = ((b >>> 17) | (b << 15)) ^ ((b >>> 19) | (b << 13)) ^ (b >>> 10);
        w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0;
      }
      let [a, b, c, d, e, f, g, hh] = h;
      for (let i = 0; i < 64; i++) {
        const S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7));
        const ch = (e & f) ^ (~e & g);
        const t1 = (hh + S1 + ch + K[i] + w[i]) | 0;
        const S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10));
        const mj = (a & b) ^ (a & c) ^ (b & c);
        const t2 = (S0 + mj) | 0;
        hh = g; g = f; f = e; e = (d + t1) | 0; d = c; c = b; b = a; a = (t1 + t2) | 0;
      }
      h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
    }
    const out = new Uint8Array(32);
    const ov = new DataView(out.buffer);
    for (let i = 0; i < 8; i++) ov.setUint32(4 * i, h[i]);
    return out;
  }

  function concat(...parts) {
    const n = parts.reduce((s, p) => s + p.length, 0);
    const out = new Uint8Array(n);
    let o = 0;
    for (const p of parts) { out.set(p, o); o += p.length; }
    return out;
  }

  function hmac(key, ...parts) {
    let k = key.length > 64 ? sha256(key) : key;
    const kp = new Uint8Array(64);
    kp.set(k);
    const ipad = kp.map((x) => x ^ 0x36);
    const opad = kp.map((x) => x ^ 0x5c);
    return sha256(concat(opad, sha256(concat(ipad, ...parts))));
  }

  function hkdf(salt, ikm, info, len) {
    const prk = hmac(salt, ikm);
    const out = new Uint8Array(len);
    let t = new Uint8Array(0);
    let pos = 0;
    for (let c = 1; pos < len; c++) {
      t = hmac(prk, t, info, new Uint8Array([c]));
      const n = Math.min(32, len - pos);
      out.set(t.subarray(0, n), pos);
      pos += n;
    }
    return out;
  }

  // ---------------------------------------------------------------- ChaCha20
  function chachaBlock(key, counter, nonce) {
    const kv = new DataView(key.buffer, key.byteOffset, 32);
    const nv = new DataView(nonce.buffer, nonce.byteOffset, 12);
    const s = new Uint32Array(16);
    s[0] = 0x61707865; s[1] = 0x3320646e; s[2] = 0x79622d32; s[3] = 0x6b206574;
    for (let i = 0; i < 8; i++) s[4 + i] = kv.getUint32(4 * i, true);
    s[12] = counter;
    for (let i = 0; i < 3; i++) s[13 + i] = nv.getUint32(4 * i, true);
    const x = s.slice();
    const qr = (a, b, c, d) => {
      x[a] += x[b]; x[d] ^= x[a]; x[d] = (x[d] << 16) | (x[d] >>> 16);
      x[c] += x[d]; x[b] ^= x[c]; x[b] = (x[b] << 12) | (x[b] >>> 20);
      x[a] += x[b]; x[d] ^= x[a]; x[d] = (x[d] << 8) | (x[d] >>> 24);
      x[c] += x[d]; x[b] ^= x[c]; x[b] = (x[b] << 7) | (x[b] >>> 25);
    };
    for (let i = 0; i < 10; i++) {
      qr(0, 4, 8, 12); qr(1, 5, 9, 13); qr(2, 6, 10, 14); qr(3, 7, 11, 15);
      qr(0, 5, 10, 15); qr(1, 6, 11, 12); qr(2, 7, 8, 13); qr(3, 4, 9, 14);
    }
    const out = new Uint8Array(64);
    const ov = new DataView(out.buffer);
    for (let i = 0; i < 16; i++) ov.setUint32(4 * i, (x[i] + s[i]) >>> 0, true);
    return out;
  }

  function chachaXor(key, counter, nonce, data) {
    const out = new Uint8Array(data.length);
    for (let o = 0, c = counter; o < data.length; o += 64, c++) {
      const ks = chachaBlock(key, c, nonce);
      const n = Math.min(64, data.length - o);
      for (let i = 0; i < n; i++) out[o + i] = data[o + i] ^ ks[i];
    }
    return out;
  }

  // ---------------------------------------------------------------- Poly1305
  const P1305 = (1n << 130n) - 5n;
  function le(bytes) {
    let v = 0n;
    for (let i = bytes.length - 1; i >= 0; i--) v = (v << 8n) | BigInt(bytes[i]);
    return v;
  }
  function poly1305(key, msg) {
    const r = le(key.subarray(0, 16)) & 0x0ffffffc0ffffffc0ffffffc0fffffffn;
    const s = le(key.subarray(16, 32));
    let acc = 0n;
    for (let o = 0; o < msg.length; o += 16) {
      const chunk = msg.subarray(o, Math.min(o + 16, msg.length));
      acc = ((acc + le(chunk) + (1n << BigInt(8 * chunk.length))) * r) % P1305;
    }
    acc = (acc + s) & ((1n << 128n) - 1n);
    const out = new Uint8Array(16);
    for (let i = 0; i < 16; i++) { out[i] = Number(acc & 0xffn); acc >>= 8n; }
    return out;
  }

  function aeadTag(key, nonce, aad, ct) {
    const otk = chachaBlock(key, 0, nonce).subarray(0, 32);
    const pad = (n) => (16 - (n % 16)) % 16;
    const mac = new Uint8Array(aad.length + pad(aad.length) + ct.length + pad(ct.length) + 16);
    mac.set(aad, 0);
    mac.set(ct, aad.length + pad(aad.length));
    const lv = new DataView(mac.buffer, mac.length - 16);
    lv.setUint32(0, aad.length, true);
    lv.setUint32(8, ct.length, true);
    return poly1305(otk, mac);
  }

  function seal(key, nonce, aad, plain) {
    const ct = chachaXor(key, 1, nonce, plain);
    return concat(ct, aeadTag(key, nonce, aad, ct));
  }

  function open(key, nonce, aad, sealed) {
    if (sealed.length < 16) return null;
    const ct = sealed.subarray(0, sealed.length - 16);
    const tag = sealed.subarray(sealed.length - 16);
    const want = aeadTag(key, nonce, aad, ct);
    let diff = 0;
    for (let i = 0; i < 16; i++) diff |= want[i] ^ tag[i];
    if (diff !== 0) return null;
    return chachaXor(key, 1, nonce, ct);
  }

  // ---------------------------------------------------------------- encoding
  function b64url(bytes) {
    let s = '';
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  }
  function unb64url(s) {
    s = s.replace(/-/g, '+').replace(/_/g, '/');
    while (s.length % 4) s += '=';
    const bin = atob(s);
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  }
  function hex(bytes) { return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join(''); }
  function unhex(s) { return Uint8Array.from(s.match(/../g) || [], (h) => parseInt(h, 16)); }

  function esc(v) { return String(v).replace(/\\/g, '\\\\').replace(/\r/g, '').replace(/\n/g, '\\n'); }
  function unesc(v) { return v.replace(/\\(.)/g, (_, c) => (c === 'n' ? '\n' : c)); }

  /** Encode {op, fields: {k: v}, blob?: Uint8Array}. */
  function encodeMsg(op, fields, blob) {
    let head = op;
    for (const [k, v] of Object.entries(fields || {})) head += '\n' + k + '=' + esc(v);
    const h = te.encode(head);
    if (!blob || !blob.length) return h;
    return concat(h, te.encode('\n\n'), blob);
  }

  function decodeMsg(p) {
    let split = -1;
    for (let i = 0; i + 1 < p.length; i++) if (p[i] === 10 && p[i + 1] === 10) { split = i; break; }
    const head = td.decode(split < 0 ? p : p.subarray(0, split));
    const blob = split < 0 ? new Uint8Array(0) : p.subarray(split + 2);
    const lines = head.split('\n');
    const fields = {};
    for (const l of lines.slice(1)) {
      const i = l.indexOf('=');
      if (i > 0) fields[l.slice(0, i)] = unesc(l.slice(i + 1));
    }
    return { op: lines[0].trim(), fields, blob };
  }

  // ---------------------------------------------------------------- session
  const AAD = te.encode('hlp1');
  function nonceFor(ctr) {
    const n = new Uint8Array(12);
    new DataView(n.buffer).setBigUint64(4, BigInt(ctr));
    return n;
  }

  class Session {
    constructor(key, cn, sn, client = true) {
      const salt = concat(cn, sn);
      const c2s = hkdf(salt, key, te.encode('hlp1 c2s'), 32);
      const s2c = hkdf(salt, key, te.encode('hlp1 s2c'), 32);
      this.tx = client ? c2s : s2c;
      this.rx = client ? s2c : c2s;
      this.txCtr = 0;
      this.rxCtr = 0;
    }
    seal(plain) {
      const ctr = this.txCtr++;
      const head = new Uint8Array(8);
      new DataView(head.buffer).setBigUint64(0, BigInt(ctr));
      return concat(head, seal(this.tx, nonceFor(ctr), AAD, plain));
    }
    open(frame) {
      if (frame.length < 24) return null;
      const ctr = Number(new DataView(frame.buffer, frame.byteOffset, 8).getBigUint64(0));
      if (ctr !== this.rxCtr) return null;
      const p = open(this.rx, nonceFor(ctr), AAD, frame.subarray(8));
      if (p) this.rxCtr++;
      return p;
    }
  }

  function randomBytes(n) {
    const b = new Uint8Array(n);
    (root.crypto || globalThis.crypto).getRandomValues(b);
    return b;
  }

  /**
   * Connect to a HydatekOS PC. `h` receives: onStatus(text, ok), onMessage(msg),
   * onFatal(reason). Returns {send(op, fields, blob), close()}.
   */
  function connect(url, key, pairId, device, h) {
    const WS = root.WebSocket || globalThis.WebSocket;
    const ws = new WS(url);
    ws.binaryType = 'arraybuffer';
    let session = null;
    let cn = null;
    const api = {
      ready: false,
      send(op, fields, blob) {
        if (!session || ws.readyState !== 1) return false;
        ws.send(session.seal(encodeMsg(op, fields, blob)));
        return true;
      },
      close() { try { ws.close(); } catch (e) { /* ignore */ } },
      get buffered() { return ws.bufferedAmount; },
    };
    ws.onopen = () => {
      cn = randomBytes(16);
      ws.send(td.decode(encodeMsg('hello', { nonce: b64url(cn), pair: pairId })));
      h.onStatus && h.onStatus('Securing connection…', false);
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === 'string') {
        const m = decodeMsg(te.encode(ev.data));
        if (m.op === 'welcome' && !session) {
          session = new Session(key, cn, unb64url(m.fields.nonce));
          api.ready = true;
          api.send('device', device);
          h.onStatus && h.onStatus('Connected to ' + (m.fields.name || 'your PC'), true);
        } else if (m.op === 'error') {
          h.onFatal && h.onFatal(m.fields.reason || 'The PC refused the connection');
        }
        return;
      }
      if (!session) return;
      const p = session.open(new Uint8Array(ev.data));
      if (!p) { api.close(); return; }
      const m = decodeMsg(p);
      if (m.op === 'ping') { api.send('pong', {}); return; }
      h.onMessage && h.onMessage(m);
    };
    ws.onclose = () => { api.ready = false; h.onClose && h.onClose(); };
    ws.onerror = () => {};
    return api;
  }

  const HLP = { sha256, hmac, hkdf, chachaBlock, poly1305, seal, open, b64url, unb64url, hex, unhex, encodeMsg, decodeMsg, Session, connect };
  if (typeof module !== 'undefined' && module.exports) module.exports = HLP;
  else root.HLP = HLP;
})(typeof window !== 'undefined' ? window : globalThis);
