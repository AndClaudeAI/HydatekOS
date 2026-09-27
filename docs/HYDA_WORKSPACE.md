# Hyda Workspace

Hyda Workspace is HydatekOS's office suite. Its first app is **Hyda Scripts**,
the word processor. Like the rest of HydatekOS, it's written from scratch and
built on no other word processor: the document model, its own file format
(`.hyds`), page layout, and the importers and exporters for other formats are
all HydatekOS code, with no third-party libraries.

![Hyda Scripts](screenshots/hyda-scripts.png)

## Hyda Scripts

Open it from the dock, the app launcher (**F1**, type "scripts"), or by
double-clicking a `.hyds` (or `.docx`) file in Files. On a phone-shaped screen it opens full
screen from Files, with the page fitted to the width.

### What it does

- **A4 pages** with 1-inch margins, laid out and paginated as you type. The status
  bar shows the page you're on, the page count and the word count.
- **Paragraph styles:** Body, Title, Heading 1, Heading 2, Quote, bulleted and
  numbered lists (numbering restarts after each list, as on screen).
- **Character formatting:** bold, italic, underline and strikethrough, on a
  selection or for what you type next.
- **Alignment:** left, centre and right.
- **Editing:** click to place the caret, drag or Shift+arrows to select,
  double-click to select a word, undo and redo (100 steps), cut, copy and paste
  (formatting survives copy and paste inside Hyda Scripts).
- **Zoom:** 50–200%, or fit to the window (click the percentage).
- **Files:** documents are saved as **`.hyds`**, Hyda Scripts' own format. Word,
  text and Markdown files can be opened and exported to (see below).

### Keyboard

| Keys | Action |
|---|---|
| Ctrl+B / Ctrl+I / Ctrl+U | Bold / italic / underline |
| Ctrl+E / Ctrl+L / Ctrl+R | Centre / left / right align |
| Ctrl+1 / Ctrl+2 / Ctrl+0 | Heading 1 / Heading 2 / Body |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+X / Ctrl+C / Ctrl+V | Cut / copy / paste |
| Ctrl+A | Select all |
| Ctrl+S / Ctrl+O / Ctrl+N | Save / open / new |
| Ctrl+= / Ctrl+- | Zoom in / out |
| Shift+arrows, Ctrl+arrows | Select; jump by word |
| Enter on an empty list item | End the list |

### Saving, importing and exporting

| | Format | What Hyda Scripts does |
|---|---|---|
| **Save** | `.hyds` | The only format it saves to your storage. **Ctrl+S** saves; a new document is named after its first line and goes in Documents. Click the name at the top to rename it. Closing the window saves your changes. |
| **Import** | `.docx`, `.txt`, `.md` | Opens the file so you can read and edit it. The status bar says "Viewing a Word file". Saving writes a **new `.hyds` next to it** (e.g. `Budget.docx` → `Budget.hyds`); the original is never changed. |
| **Export** | `.docx`, `.txt`, `.md` | File › Export writes a copy for sharing, next to the document. It never replaces an existing file; the document you're editing stays `.hyds`. |

Exported Word files are standard Office Open XML, so the people you send them to
can open them in Word or any other word processor.

When importing Word files, Hyda Scripts reads paragraphs, headings (by style
name, in any language), lists, alignment, bold, italic, underline, strikethrough
and tabs. **It doesn't bring in yet:** images, tables (their text appears as
paragraphs), fonts, font sizes and colours, headers and footers, footnotes,
comments, tracked changes and page setup. Because the original file is never
overwritten, none of that is lost.

## The .hyds format

A `.hyds` file is UTF-8 text with one record per line. It's designed for
HydatekOS: easy to read, easy to recover, and checked for damage.

```
HYDS 1                          magic and format version
app Hyda Scripts                the program that wrote it
paras 3                         number of paragraphs
p title left                    a paragraph: style, alignment
t Budget 2026                   its text
p body center
t Total: ₦1,200,000 approved.
f 7:10:b 17:10:i                formatting runs: start:length:flags
p bullet left
t Rent
end 44f68f84                    CRC-32 of every byte before this line
```

- **Styles:** `body`, `title`, `h1`, `h2`, `quote`, `bullet`, `number`.
  **Alignment:** `left`, `center`, `right`.
- **Text (`t`):** the paragraph's characters; `\\` is a backslash and `\t` a tab.
- **Formatting (`f`):** runs of characters, counted from 0, with flags `b` bold,
  `i` italic, `u` underline, `s` strikethrough. Unformatted text has no run.
- **Integrity:** `end` carries the CRC-32 (hex) of everything before it. A file
  with a wrong checksum, a missing `end` line or a wrong paragraph count is
  reported as damaged or incomplete instead of being opened half-read.
- **Versions:** `HYDS 1` is this version. Readers ignore record types they don't
  know, so later versions can add records (colours, images...) that older
  readers skip. A file with a newer major version is refused with a clear message.

### Other limits

- **Italic is a slanted version** of the regular font (the Figtree family ships no
  italics), so it looks slightly different from a designed italic.
- **One font, fixed sizes per style.** No font picker or size box yet.
- **No spelling check, find and replace, images, tables or printing yet.**
- **Phone Link paste:** text copied on a paired phone isn't pasted into Hyda
  Scripts yet.

## How it's built

| Part | Where |
|---|---|
| Document model, editing, the `.hyds` format, `.docx`/`.txt`/`.md` import and export | `kernel/src/doc.rs` |
| Zip reading and writing, CRC-32, inflate (RFC 1951) | `kernel/src/zip.rs` |
| The app: layout, pagination, rendering, input | `kernel/src/apps/scripts.rs` |
| Italic faces (slanted at build time), ₦ and other symbols | `tools/fontgen.py` |

A document is a list of paragraphs; each has a style, an alignment, its
characters and a formatting byte per character (bold, italic, underline,
strikethrough). Layout measures each character with its face and size, wraps at
spaces, keeps headings with the line after them, and flows lines onto A4 pages.

### Tests

`tools/test.sh` runs these on the host (`tests-host/src/doc_tests.rs`):

- inflate against zlib's output (stored, fixed-Huffman and dynamic-Huffman blocks)
- zip round trip and CRC-32
- editing: typing, Enter, deleting across paragraphs, formatting, cut and paste
- `.hyds` round trip (including backslashes, tabs, leading spaces, ₦), and
  rejecting other files, newer versions, truncated and corrupted files;
  skipping unknown records
- `.docx` round trip and Markdown round trip
- importing Word files written by other programs (a file re-saved by another
  word processor, and one built on Word's default template)

Those sample files, and the other word processor used to check that exported
`.docx` files open correctly, are test tools on the build machine only; nothing
from them is part of HydatekOS.
