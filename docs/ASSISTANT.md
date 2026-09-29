# Claude, the HydatekOS assistant

HydatekOS's assistant is **Claude**, made by Anthropic. HydatekOS doesn't use or
offer any other assistant. Claude answers in its own app (first in the dock, or
**Gen+Space** and type *Claude*). It can explain how to do things in HydatekOS,
write and rewrite text, plan, summarise and answer questions.

| Meeting Claude in the setup assistant | A new conversation | Settings › Assistant |
|---|---|---|
| ![Setup](screenshots/assistant-setup.png) | ![Claude](screenshots/assistant-chat.png) | ![Settings](screenshots/assistant-settings.png) |

## Setting it up

Claude runs on Anthropic's API, and each account uses its own **Anthropic API
key**:

1. Make a key at [console.anthropic.com](https://console.anthropic.com), under
   **API keys**. Keys start with `sk-ant-`.
2. Paste it with **Gen+V**:
   - in the setup assistant's **Meet Claude** step, or
   - in the Claude app, or
   - in **Settings › Assistant**.

The setup step can be skipped. The key can be changed or removed any time in
Settings › Assistant.

## Using it

| Action | How |
|---|---|
| Ask something | Type in *Ask Claude* and press **Enter** |
| Stop waiting for an answer | **Esc**, or the square button |
| Start again | **New chat**, or **Gen+N** |
| Scroll back | Mouse wheel, **Page Up** / **Page Down** |
| Try a failed question again | **Try again** under the conversation |

Every text box uses HydatekOS's line editor, so ← → (Gen: by word), Home, End,
Gen+Backspace and Gen+V all work as they do everywhere else.

Answers show as plain text. Claude is asked not to use Markdown, and any
headings, bold or code fences that still come through are tidied away.

## The model

Claude uses **the newest Opus model your key can use**. The first time you ask
something, HydatekOS reads the list of models from Anthropic's Models API and
picks from it. No model name is built into HydatekOS, so new models are picked up
as they arrive. **Settings › Assistant › Choose…** lists the models on your key
if you'd rather use another. If a chosen model is retired, HydatekOS picks again
by itself.

## Privacy

- The key is stored in your account's system folder (`assistant.txt` under
  `/system`, or `/system/users/<account>` for later accounts). Apps, the
  Terminal and other accounts can't read it.
- A request contains only what you type in the Claude app, the conversation so
  far, your name and today's date. Nothing else on the computer is sent: no
  files, no other apps' contents, no screen.
- Requests go straight to `api.anthropic.com` over HydatekOS's own TLS, checked
  against its built-in certificate authorities. Nothing goes through a
  HydatekOS server.
- Conversations live in memory only. **New chat**, closing the app or
  signing out forgets them.

## How it works

- `kernel/src/web/claude.rs` builds Messages API requests:
  - the `x-api-key` and `anthropic-version` headers;
  - a system prompt that tells Claude it is the assistant built into HydatekOS,
    which apps exist and that answers appear as plain text;
  - the conversation, up to the last 60 turns.

  It also reads answers, stop reasons (`max_tokens` and `refusal` get a note)
  and API errors, which become sentences such as "The API key wasn't
  accepted".
- `kernel/src/web/json.rs` is a small JSON parser and string writer.
- The fetcher (`kernel/src/web/fetch.rs`) carries the requests like any web
  page, with two additions:
  - extra headers, which aren't passed on through redirects to other sites;
  - a five-minute patience for long answers.
- `kernel/src/apps/assistant.rs` is the app; `kernel/src/shell/setup.rs` has the
  setup step.
- The host tests (`tests-host/src/claude_tests.rs`) check:
  - the request body;
  - how answers, refusals, cut-off answers and errors are read;
  - model choice and key checks;
  - that headers can't inject lines into a request.
- In QEMU, a request with a made-up key went to `api.anthropic.com` over TLS and
  came back with the API's `401`, shown as "The API key wasn't accepted".
