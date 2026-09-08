# Android Dashboard

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![Rust](https://img.shields.io/badge/rust-1.88.0-orange?logo=rust)
![macOS](https://img.shields.io/badge/platform-macOS-black?logo=apple)
[![CI](https://github.com/GunnarKarlsson/android-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/GunnarKarlsson/android-tui/actions/workflows/ci.yml)
[![Stars](https://img.shields.io/github/stars/GunnarKarlsson/android-tui)](https://github.com/GunnarKarlsson/android-tui/stargazers)

A native GUI written in Rust for debugging Android devices and emulators on macOS.

![Android Terminal dashboard](screenshot1.png)

Select a device, then watch RAM, storage, logcat, and network in one window. Panels resize by dragging the gaps between them.

## Panels:

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

## Configure AI provider

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

The provider must accept [OpenAI Chat Completions](https://platform.openai.com/docs/api-reference/chat/create): `POST {AI_PROVIDER_BASE_URL}/chat/completions` (include `/v1` in the base URL if that is part of the path):

```http
POST /chat/completions
Authorization: Bearer ${AI_PROVIDER_API_KEY}
Content-Type: application/json

{
  "model": "${AI_PROVIDER_MODEL}",
  "temperature": 0.2,
  "max_tokens": 350,
  "stream": false,
  "thinking": { "type": "disabled" },
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "<error snapshot JSON>" }
  ]
}
```

`"thinking"` is sent for DeepSeek; other hosts typically ignore unknown fields.

Response:

```json
{
  "choices": [
    {
      "message": {
        "role": "assistant",
        "content": "FAILING\nTop issues: ...\nNext checks: ..."
      }
    }
  ]
}
```

## Prerequisites

- **Rust** 1.88+ (see `rust-toolchain.toml`)
- **Android SDK platform-tools** with `adb` on your `PATH`

Install platform-tools via [Android Studio](https://developer.android.com/studio) and verify adb is available:
```
adb version
```
You'll need a device connected, via USB or via emulator.


## Run the Dashboard

```bash
cargo run -p android-terminal
```

## Project layout

```
crates/
  adb-client/       # adb command wrappers and parsing
  ai-insight/       # error clustering and Chat Completions client
  android-terminal/ # GUI application
```
