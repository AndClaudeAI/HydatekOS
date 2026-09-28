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
- **HTTP:** redirects, cookies, chunked transfer, gzip / deflate, and a 30-second
  timeout. Files that aren't web pages are saved to Downloads.

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

## Not yet

- **Secure sites (https).** Almost every site today uses HTTPS, which needs TLS
  (encryption and certificate checks). HydatekOS's TLS is the next step; until
  then the browser opens `http://` sites and says so for `https://` ones.
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
| The browser app and its built-in pages | `kernel/src/apps/browser.rs` |

`tools/test.sh` tests URLs, DNS messages, HTTP parsing (byte by byte, chunked,
gzip), HTML parsing, CSS and layout, and search ranking and storage on the host.
In QEMU, the browser was checked against a local test site: pages, links,
tables, forms (a gzip + chunked reply), and crawling and searching it.
