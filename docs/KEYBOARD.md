# The keyboard: Gen and Aux

HydatekOS names its two modifier keys after what they do, not after another
system's keyboards:

| HydatekOS | What it does | On a PC keyboard | On an Apple keyboard | Windows / macOS call it |
|---|---|---|---|---|
| **Gen** (general) | Shortcuts: Gen+S saves, Gen+C copies | the ⊞ logo key, and Ctrl | ⌘ Command, and Control | Ctrl / Command |
| **Aux** (auxiliary) | Special characters, switching windows, moving by word | Alt | ⌥ Option | Alt / Option |

![Settings › Keyboard](screenshots/keyboard-settings.png)

## Gen

Hold Gen and press a letter. Menus show each command's shortcut on the right,
and **Gen+/** shows all of them:

| Menus show shortcuts | Gen+/ |
|---|---|
| ![A menu with shortcuts](screenshots/keyboard-menu.png) | ![Keyboard shortcuts](screenshots/keyboard-shortcuts.png) |

Gen is the logo key: ⊞ on PC keyboards, or ⌘ Command on an Apple keyboard,
where Command sits in the same place. **Ctrl works as Gen too**, so Windows
habits carry over. In Settings › Keyboard you can switch that off, and Ctrl
then belongs to the apps. The Terminal uses it as a Unix shell does:
**Ctrl+C** cancels the line, **Ctrl+L** clears the screen and **Ctrl+U**
clears what you've typed. Everywhere else, a Ctrl+letter that means nothing
types nothing. The setting is kept for each account.

### Everywhere

| Keys | Action |
|---|---|
| Gen+Space | Open an app (the launcher). F1 does too |
| Gen+Tab | Next window; with Shift, the one before. Aux+Tab does the same |
| Gen+W | Close the window |
| Gen+Q | Quit the app |
| Gen+, | Settings |
| Gen+/ | Keyboard shortcuts |
| F12 | Lock the screen |

Window switching needs the logo key or Aux: some firmware reports Ctrl+I as
Tab, and Ctrl+Tab has to stay Gen+I (italic) there.

### In apps

| Keys | Action |
|---|---|
| Gen+N / Gen+O / Gen+S | New / open / save |
| Gen+Z / Gen+Y | Undo / redo |
| Gen+X / Gen+C / Gen+V | Cut / copy / paste |
| Gen+A | Select all |
| Gen+B / Gen+I / Gen+U | Bold / italic / underline |
| Gen+L / Gen+E / Gen+R | Left / centre / right (Hyda Scripts and Slides) |
| Gen+M, Gen+D | New slide, duplicate (Hyda Slides) |
| Gen+L, Gen+R | Address bar, reload (Browser) |
| Gen+V | Paste into the command line (Terminal) |

Every app's own list is in [HYDA_WORKSPACE.md](HYDA_WORKSPACE.md) and
[BROWSER.md](BROWSER.md). Gen+Shift+letter counts as Gen+letter.

## Aux

Hold Aux and press a key to type a special character. The layout follows the
Mac's Option key where it can (Aux+3 £, Aux+8 •, Aux+; …, Aux+- –), and
Aux+N types the Naira sign.

| Key | Aux | Aux+Shift | | Key | Aux | Aux+Shift |
|---|---|---|---|---|---|---|
| N | ₦ | ₦ | | - | – | — |
| E | € | € | | ; | … | |
| 3 | £ | | | [ | “ | ” |
| Y | ¥ | ¥ | | ] | ‘ | ’ |
| 4 | ¢ | | | \ | « | » |
| R | ® | ₹ | | 1 | ¡ | |
| C | | ₵ | | / | ÷ | ¿ |
| G | © | © | | = | ≠ | ± |
| 2 | ™ | | | , | ≤ | |
| 6 / 7 | § / ¶ | | | . | ≥ | |
| 8 | • | ° | | X | × | × |
| 0 | ° | | | W | Σ | Σ |
| M | µ | µ | | L | ¬ | ¬ |

![Typing with Aux](screenshots/keyboard-aux.png)

Aux with a key that has no character types nothing. Aux also has two jobs
besides characters:
- **Aux+Tab** switches windows.
- **Aux+← and Aux+→** move by word in text, as Gen+← and Gen+→ do.

The same characters, and more, are on the on-screen keyboard's **₦€£** page on
touch screens.

## How it's built

| Part | Where |
|---|---|
| Reading the modifiers from the firmware; Gen, Ctrl and Aux | `kernel/src/input.rs` (`gen_key`) |
| What Aux types | `kernel/src/keymap.rs` |
| Global shortcuts, window switching, the shortcuts sheet, menu hints | `kernel/src/shell/mod.rs`, `shell/keys.rs` |
| Settings › Keyboard | `kernel/src/apps/settings.rs` |
| ⊞ ⌘ ⌥ ↑ ↓ − in the font | `tools/fontgen.py` |

Apps receive `key(k, gen, sys)`: `gen` is true while Gen is held. `Key::Ctrl`
(Ctrl while it isn't Gen) and `Key::Aux` (Aux with no character) exist so that
neither ever types a letter by accident.

`tools/test.sh` checks what Aux types (Shift, capitals, keys with nothing) and
that every character in the Aux table is in HydatekOS's font
(`tests-host/src/keymap_tests.rs`). The firmware's reports were checked in
QEMU: the logo key, Ctrl, Alt and Shift each arrive as their own flag with the
key. So were the shortcuts, the sheet, the menu hints, window switching, typing
with Aux, and the Terminal's Ctrl keys with Ctrl not working as Gen.
