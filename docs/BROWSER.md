# Browser and Hyda Search

HydatekOS has its own web browser and its own search engine, **Hyda Search**.
Both are written from scratch for HydatekOS: the TCP connections and DNS
resolver, the HTTP client, the HTML parser, CSS and page layout, and the search
index and crawler. None of it comes from another browser or search engine.

![A page in the HydatekOS browser](screenshots/browser-page.png)

## The browser

- **Address bar:** type an address (`example.com`, `http://10.0.2.2:8080/`) or
  words to search. Ctrl+L selects the address.
- **Back, forward, reload / stop, home** (Hyda Search), a loading bar, and the
  link under the pointer in the status bar. Backspace goes back; Space and Page
  Up/Down scroll.
- **Pages:** headings, paragraphs, bold / italic / underline / strikethrough,
  links (including `#section` links), bulleted and numbered lists, block quotes,
  preformatted text, simple tables, horizontal rules, and colours, backgrounds,
  borders, sizes, alignment, margins, padding and widths from CSS.
- **CSS:** `<style>` blocks and `style=""` attributes, with type, class, id,
  descendant and child selectors, specificity, and `@media` rules for wide
  screens.
- **Forms:** text fields, password fields, checkboxes, buttons and drop-downs
  (their selected value); GET and POST.
- **HTTP and HTTPS:** redirects, cookies, chunked transfer, gzip / deflate, and
  a 30-second timeout. Files that aren't web pages are saved to Downloads.
- **Secure sites:** a padlock in the address bar; click it for how the
  connection is secured. Plain `http://` pages get an ⓘ that says they aren't.

![An article](screenshots/browser-article.png)

## Hyda Search

![Hyda Search results](screenshots/hyda-search.png)

Hyda Search is the browser's home page. It searches:

- **the pages you visit** (each page you open is added to the index),
- **sites you add:** type a site into "Add a site" and the crawler reads it,
- **your files:** documents, sheets, notes and text files in your home folder.

Results are ranked with **BM25** (the ranking function used by most search
engines), with words in a page's title counting extra and pages that contain all
your words first. Each result shows a snippet with your words in bold. Plural
and singular forms match each other.

![Hyda Search finding a file](screenshots/hyda-search-files.png)

**The crawler is polite:** it reads a site's `robots.txt` first and follows its
rules, fetches one page per second, stays on the site you added, follows links
up to three levels deep, reads at most 60 pages per site, and skips pages marked
`noindex`. **Manage your index** lists the sites in it and removes any of them.

**Private:** the index is stored on this computer (`/system/search.hydx`), and
searches never leave it. There is no account and no tracking.

**What it isn't:** a search engine for the whole web. Companies that index the
web run warehouses of computers crawling billions of pages; Hyda Search knows
the pages you visit, the sites you add and your files.

## Secure connections (HTTPS)

![The connection details of a secure site](screenshots/browser-https.png)

HydatekOS has its own TLS, written from scratch like the rest:

- **TLS 1.3** (RFC 8446) and **TLS 1.2** with ECDHE (RFC 5246, with extended
  master secret), no older versions. A TLS 1.2 answer from a server that could
  do 1.3 is caught (the downgrade sentinel).
- **Key exchange:** X25519, or P-256 / P-384 when the server asks for them
  (TLS 1.3's HelloRetryRequest).
- **Ciphers:** AES-128-GCM, AES-256-GCM and ChaCha20-Poly1305.
- **Signatures:** RSA (PSS and PKCS #1 v1.5, 2048 bits and up) and ECDSA on
  P-256 and P-384, with SHA-256, SHA-384 and SHA-512.
- **Certificates:** the browser follows the site's certificate chain up to one
  of **146 trusted authorities** (Mozilla's CA list, the one Firefox and most
  Linux systems use, stored in `kernel/src/tls/roots.bin`; `tools/rootgen.py`
  rebuilds it). It checks every signature, the dates, that authorities are
  marked as authorities, and that the certificate names the site (including
  `*.example.com` names and IP addresses).

When something is wrong, the page doesn't open:

![A certificate HydatekOS doesn't trust](screenshots/browser-not-private.png)

**Adding an authority:** organisations with their own certificate authority
(or a network that inspects traffic) can put its certificate, as `.pem`,
`.crt` or `.der`, in `/system/certs`; HydatekOS trusts it from the next start.

**Checked against:** OpenSSL's server in 27 handshakes (both TLS versions,
every cipher, RSA, P-256 and P-384 certificates, chains with an intermediate,
retry requests, a server asking for a client certificate, a 200 KB download),
and a production TLS 1.3 server on the internet (below: pypi.org, reached
through the test machine's security gateway, whose authority was added as
above; without it HydatekOS refused the connection, as it should). The
crypto is checked against published test vectors and 118 of Mozilla's root
certificates, whose own signatures HydatekOS verifies.

![pypi.org over TLS 1.3](screenshots/browser-pypi.png)

**Limits:** no certificate revocation checks (OCSP, CRLs) or Certificate
Transparency, no session resumption (every page is a full handshake), no
name constraints, and ECDSA/RSA code that isn't constant-time (it only
handles public data, except the P-256/P-384 key exchange, which is used when
a server refuses X25519).

## Not yet

- **Images, JavaScript, web fonts and external stylesheets** (`<link rel=stylesheet>`).
- **Tabs, bookmarks, find in page, downloads list, and text selection on pages.**
- **Layout:** floats, flexbox and grid are laid out as ordinary blocks, so
  multi-column pages stack their columns.

## How it's built

| Part | Where |
|---|---|
| TCP connections (active open), UDP for DNS | `kernel/src/net/` |
| URLs, DNS messages, HTTP messages | `kernel/src/web/url.rs`, `dns.rs`, `http.rs` |
| Requests from apps, run by the main loop | `kernel/src/web/mod.rs`, `fetch.rs` |
| HTML parser | `kernel/src/web/html.rs` |
| CSS, style and layout | `kernel/src/web/render.rs` |
| Hyda Search: index, ranking, crawler | `kernel/src/web/search.rs` |
| TLS: hashes, AES-GCM, X25519, P-256/P-384, RSA, X.509, the handshake | `kernel/src/tls/` |
| The browser app and its built-in pages | `kernel/src/apps/browser.rs` |

`tools/test.sh` tests URLs, DNS messages, HTTP parsing (byte by byte, chunked,
gzip), HTML parsing, CSS and layout, search ranking and storage, and TLS
(crypto vectors, certificates, and handshakes with `openssl s_server`) on the
host. `HYDATEK_LIVE=example.com cargo test --release live -- --ignored` in
`tests-host` connects to a real site.
In QEMU, the browser was checked against a local test site over HTTP and
HTTPS: pages, links, tables, forms (a gzip + chunked reply), crawling and
searching it, and refusing an untrusted certificate.
