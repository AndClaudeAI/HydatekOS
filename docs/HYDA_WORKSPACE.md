# Hyda Workspace

Hyda Workspace is HydatekOS's office suite: **Hyda Scripts**, the word
processor, and **Hyda Grids**, the spreadsheet. Like the rest of HydatekOS, it's written from scratch and
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

## Hyda Grids

![Hyda Grids](screenshots/hyda-grids.png)

Hyda Grids is the spreadsheet. Open it from the dock, the app launcher (**F1**,
type "grids"), or by double-clicking a `.hydg` (or `.xlsx` / `.csv`) file in Files.
A starter sheet, *Budget*, is in Documents.

### What it does

- **A grid of 1,000 rows by 100 columns** (A to CV) with frozen row and column
  headers. The active cell and its row and column headers are highlighted.
- **Formulas** start with `=`: `=SUM(B2:B5)`, `=D2*7.5%`, `=IF(C4>100,"over","ok")`,
  `=A1&" "&B1`. Operators: `+ - * / ^ %`, `&` (join text), `= <> < > <= >=`.
  Absolute references use `$` (`$B$2`).
- **Functions:** SUM, AVERAGE, MIN, MAX, COUNT, COUNTA, PRODUCT, ROUND, INT, ABS,
  SQRT, POWER, MOD, IF, IFERROR, AND, OR, NOT, CONCAT, LEN, UPPER, LOWER, TRIM,
  SUMIF and COUNTIF.
- **Errors** show in the cell: `#DIV/0!`, `#VALUE!`, `#REF!`, `#NAME?`, `#NUM!`,
  and `#CIRC!` for a formula that depends on itself.
- **Number formats:** General, Number (1,234.50), Currency (₦1,234.50) and
  Percent. Typing `₦5,000`, `12%` or `1,250.5` picks the format for you; start
  with `'` to keep something as text (`'007`).
- **Formatting:** bold, italic, left / centre / right alignment. Numbers align
  right by themselves; long text runs on into empty cells to the right.
- **Editing:** type to replace a cell, **F2** or double-click to edit it, or edit
  in the formula bar. While typing a formula, clicking a cell inserts its
  reference.
- **Selecting:** click, drag, Shift+click, Shift+arrows, or click a row or column
  header. The status bar shows the **sum, average and count** of the selection.
- **Copy and paste** move formulas' relative references (`=D2*0.075` copied one
  row down becomes `=D3*0.075`). Text copied from elsewhere pastes into cells,
  split by tabs and lines.
- **AutoSum (Σ):** adds up the numbers above (or to the left of) the cell, in
  their number format; with a range selected, it totals each column underneath.
- **Columns:** drag a column header's edge to resize it; double-click the edge to
  reset it.
- **Undo and redo** (100 steps).

### Keyboard

| Keys | Action |
|---|---|
| Arrows, Tab, Enter (Shift for the other way) | Move |
| Shift+arrows | Select |
| F2 / Esc | Edit the cell / cancel editing |
| Delete / Backspace | Clear / clear and edit |
| Ctrl+B / Ctrl+I | Bold / italic |
| Ctrl+X / Ctrl+C / Ctrl+V | Cut / copy / paste |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+A | Select everything |
| Ctrl+Home / Ctrl+End | First cell / last used cell |
| Ctrl+S / Ctrl+O / Ctrl+N | Save / open / new |

### Saving, importing and exporting

| | Format | What Hyda Grids does |
|---|---|---|
| **Save** | `.hydg` | The only format it saves to your storage. A new sheet goes to Documents; click the name at the top to rename it. Closing the window saves your changes. |
| **Import** | `.xlsx`, `.csv` | Opens the file to read and edit ("Viewing an Excel file"). Saving writes a **new `.hydg` next to it**; the original is never changed. |
| **Export** | `.xlsx`, `.csv` | File › Export writes a copy for sharing: an Excel workbook with formulas, their results, formats and column widths, or a CSV of the values. |

When importing Excel files, Hyda Grids reads the first sheet's values, formulas
(including shared formulas), bold and italic, alignment, currency / percent /
number formats and column widths. Dates are shown as text (`2026-09-27`).

### The .hydg format

Line-based UTF-8 text, like `.hyds`:

```
HYDG 1                          magic and format version
app Hyda Grids                  the program that wrote it
sheet Budget                    sheet name
w 0 140                         column A is 140 px wide
c A1 b Item                     cell: reference, format, input
c B2 n=cur,sym=₦ 12000
c D2 n=cur,sym=₦ =B2+C2
c C7 i,n=pct =C6/B6-1
end f80f9715                    CRC-32 of every byte before this line
```

- **Format** is `-` or a comma list of `b` (bold), `i` (italic), `al=l|c|r`,
  `n=num|cur|pct` and `sym=` (the currency symbol).
- **Input** is exactly what was typed (a value, or a formula starting with `=`);
  `\\`, `\t` and `\n` escape a backslash, tab and newline.
- Checksum, versions and unknown records work as in `.hyds`.

### Limits

- **One sheet per file.** Importing an Excel workbook reads its first sheet.
- **Not yet:** inserting or deleting rows and columns, sorting and filtering,
  charts, cell colours and borders, freezing panes, find and replace, dates and
  times as values, and printing.
- Excel functions Hyda Grids doesn't have show `#NAME?` (the formula is kept).

## How it's built

| Part | Where |
|---|---|
| Document model, editing, the `.hyds` format, `.docx`/`.txt`/`.md` import and export | `kernel/src/doc.rs` |
| Zip reading and writing, CRC-32, inflate (RFC 1951) | `kernel/src/zip.rs` |
| The app: layout, pagination, rendering, input | `kernel/src/apps/scripts.rs` |
| Hyda Grids: cells, formula parser and evaluator, number formats | `kernel/src/grid.rs` |
| Hyda Grids files: `.hydg`, `.xlsx` import/export, CSV | `kernel/src/gridio.rs` |
| The Hyda Grids app | `kernel/src/apps/grids.rs` |
| Italic faces (slanted at build time), ₦, Σ and other symbols | `tools/fontgen.py` |

A document is a list of paragraphs; each has a style, an alignment, its
characters and a formatting byte per character (bold, italic, underline,
strikethrough). Layout measures each character with its face and size, wraps at
spaces, keeps headings with the line after them, and flows lines onto A4 pages.

### Tests

`tools/test.sh` runs these on the host (`tests-host/src/doc_tests.rs` and
`grid_tests.rs`). For Hyda Grids: references, operator precedence, every
function, circular references, number formats and typed values, moving
references, `.hydg` round trip and damage checks, CSV, `.xlsx` round trip, an
Excel file written by another program, and shared formulas. For Hyda Scripts:

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
