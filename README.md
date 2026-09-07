# Android Terminal

A native GUI written in Rust for debugging Android devices and emulators on macOS.

![Android Terminal dashboard](screenshot1.png)

Select a device, then watch RAM, storage, logcat, and network in one window. Panels resize by dragging the gaps between them.

Panels:

**Devices** — Connected emulators and USB devices. Click one to drive the rest of the dashboard. Footer **Refresh** re-runs `adb devices`. Offline or unauthorized entries are listed but cannot be selected.

**RAM** — Live memory usage

**Storage** — Live internal storage

**Logcat** — Streaming logcat for the selected device.

- Substring filter
- Tag filters (type a tag and press Enter)
- Footer **Stream** pauses the feed
- Footer **Timestamp** toggles timestamps
- Lines colored by log level

**Logcat Errors** — Logcat but only errors and fatals.

**Insight** — Short AI verdict on recent errors. See [AI insights](#ai-insights).

**Storage Details** — Category totals and per-app storage.

**Network Activity** — Per-interface RX/TX totals and current down/up rates.

**App Traffic** — Per-package network usage: total, foreground, background, WiFi, and mobile.

## AI insights

Error and fatal logcat lines are clustered by tag and a noise-stripped message shape (hex, paths, and numbers are stripped, and security-related data is redacted). The app POSTs a JSON snapshot of those clusters to an LLM. When the mix of errors changes, it posts again (with a cooldown).

The model replies with a one-line verdict (`HEALTHY` / `DEGRADING` / `FAILING`), top issues, and a few next checks. That text shows in the **Insight** panel.

No request is sent until `AI_PROVIDER_API_KEY` is set.

## Configure an AI provider

Settings are read from the process environment. On startup the app also loads `crates/android-terminal/.env` if that file exists.

| Variable | Required | Default |
| --- | --- | --- |
| `AI_PROVIDER_API_KEY` | yes | (empty — Insight is skipped) |
| `AI_PROVIDER_BASE_URL` | no | `https://api.deepseek.com` |
| `AI_PROVIDER_MODEL` | no | `deepseek-v4-pro` |

Example `.env`:

```
AI_PROVIDER_API_KEY=sk-...
AI_PROVIDER_BASE_URL=https://api.deepseek.com
AI_PROVIDER_MODEL=deepseek-v4-pro
```

Any provider that implements the [OpenAI Chat Completions](https://platform.openai.com/docs/api-reference/chat/create) HTTP API works: set `AI_PROVIDER_BASE_URL` to the origin that serves `POST /chat/completions` (include `/v1` if that is part of the path), `AI_PROVIDER_API_KEY` to that provider’s bearer token, and `AI_PROVIDER_MODEL` to a model id it accepts.

The request is Chat Completions JSON: `Authorization: Bearer …`, body fields `model`, `messages` (`system` / `user`), `temperature`, `max_tokens`, `stream: false`. The assistant text is read from `choices[0].message.content`. The body also sends `"thinking": { "type": "disabled" }` (DeepSeek); other hosts typically ignore unknown fields.

## Prerequisites

- **Rust** 1.88+ (see `rust-toolchain.toml`)
- **Android SDK platform-tools** with `adb` on your `PATH`

Install platform-tools via [Android Studio](https://developer.android.com/studio) or the [SDK command-line tools](https://developer.android.com/studio#command-tools), then verify:

```bash
adb version
```

## Run

```bash
cargo run -p android-terminal
```

An Android emulator or USB-connected device must be running and authorized (`adb devices` should list it as `device`).

## Project layout

```
crates/
  adb-client/       # adb command wrappers and parsing
  ai-insight/       # error clustering and Chat Completions client
  android-terminal/ # GUI application
```
