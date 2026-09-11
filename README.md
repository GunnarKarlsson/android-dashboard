# Android Debug Dashboard

![Rust](https://img.shields.io/badge/rust-1.88.0-orange?logo=rust)
![macOS](https://img.shields.io/badge/platform-macOS-black?logo=apple)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![CI](https://github.com/GunnarKarlsson/android-dashboard/actions/workflows/ci.yml/badge.svg)](https://github.com/GunnarKarlsson/android-dashboard/actions/workflows/ci.yml)
[![Stars](https://img.shields.io/github/stars/GunnarKarlsson/android-dashboard)](https://github.com/GunnarKarlsson/android-dashboard/stargazers)

A dashboard written in Rust for debugging Android apps on macOS. 
Shows essential Android debug data from hardware devices and emulator in a single window, together with LLM-generated insights into the error log.

![Android Terminal dashboard](screenshot1.png)

## Prerequisites

- **Rust** 1.88+ (see `rust-toolchain.toml`)
- **Android SDK platform-tools** with `adb` on your `PATH`

Install platform-tools via [Android Studio](https://developer.android.com/studio) and verify adb is available:

```bash
adb version
```
You'll need an Android device connected, via USB or via emulator.

## Build

```bash
cargo build
```

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

## How to Use the Dashboard

In the dashboard's upper left devices widget, select the device you want to inspect. This will populate the dashboard with the device's data. The first device in the device list is automatically selected on start. The device list is ordered as per `adb devices -l`.

## Widgets

The dashboard shows the following data in widgets:

| Widget | Description |
| --- | --- |
| Devices | Connected emulators and USB devices. |
| RAM | Live memory usage. |
| Storage | Live internal storage. |
| Logcat | Streaming logcat for the selected device. Filter by tag. Pause and timestamps are independent of Logcat Errors. |
| Logcat Errors | The same live stream, Error and Fatal only. Tag filter, pause, and timestamps are independent of Logcat. |
| Insight | An LLM's opinion on the error logs. See [AI insights](#ai-insights) for details. |
| Storage Details | Directory totals and per-app storage. |
| Network Activity | Per-interface RX/TX totals and current down/up rates. |
| App Traffic | Network usage by app/package. |

## AI insights

The dashboard app submits a normalized logcat error log to a Chat Completions API of your choice, and displays the response.
Before dispatch to the API, error and fatal logcat lines are summarized, noise-stripped and filtered to remove secret data. 

When the mix of errors changes, the app posts again, with a cooldown.

The model replies with a one-line verdict (`HEALTHY` / `DEGRADING` / `FAILING`), top issues, and a recommendation for what to do next. That text shows in the **Insight** panel.

No request is sent until base URL, model, and API key are set via **Configure AI Provider** (cog menu ⚙️).

## Configuration

Insight is optional. Configure the provider from the macOS cog menu: **Configure AI Provider**. Settings are stored as `insight.json` next to `layout.json` under the OS config directory (`dirs::config_dir()/android-terminal/`). On macOS that is typically `~/Library/Application Support/android-terminal/insight.json`.

| Field | Required |
| --- | --- |
| Base URL | yes |
| Model | yes |
| API key | yes |

Example values:

```
Base URL: https://api.deepseek.com
Model: deepseek-v4-pro
API key: sk-...
```

The provider should accept [OpenAI Chat Completions](https://platform.openai.com/docs/api-reference/chat/create): `POST {base_url}/chat/completions` (include `/v1` in the base URL if that is part of the path):

```http
POST /chat/completions
Authorization: Bearer <api_key>
Content-Type: application/json

{
  "model": "<model>",
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

The `"thinking"` field is sent for DeepSeek. Other hosts typically ignore unknown fields.

Expected response format from API:

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

## Layout Persistence

Panel layout is saved as `layout.json` under the OS config directory (`dirs::config_dir()/android-terminal/`). On macOS that is typically `~/Library/Application Support/android-terminal/layout.json`.

If the file is missing or invalid, the app uses a default three-column layout. **Reset Dashboard Layout** in the cog menu restores that default and overwrites the file.

## Commands Used by App

Every subprocess the app starts runs `adb` on `PATH`. Device-specific commands are always `adb -s <serial> …`. All `adb -s` shells share one mutex; poller threads do not run shells in parallel.

### Host / session

| Command | When |
| --- | --- |
| `adb version` | On startup |
| `adb devices -l` | On startup, then On refresh in the devices panel |
| `adb -s <serial> shell getprop ro.product.model` | While app is building device list, only if `devices -l` had no `model:` field |

### While a device is selected

Selecting a device starts one logcat process plus several pollers.

Streaming (stays running until the device is deselected):

- `adb -s <serial> logcat -v threadtime` — full logcat; Logcat Errors is a view of this stream (Error/Fatal only)


| Command | Interval | Used for |
| --- | --- | --- |
| `adb -s <serial> shell cat /proc/meminfo` | 2s (backoff to 5s on error) | RAM gauge |
| `adb -s <serial> shell df -k /storage/emulated/0` | 10s (backoff to 30s) | Storage donut totals |
| `adb -s <serial> shell du -sb /storage/emulated/0/DCIM /storage/emulated/0/Pictures /storage/emulated/0/Movies /storage/emulated/0/Music /storage/emulated/0/Podcasts /storage/emulated/0/Audiobooks /storage/emulated/0/Ringtones /storage/emulated/0/Documents /storage/emulated/0/Download /storage/emulated/0/Android` | 30s | Storage category breakdown |
| `adb -s <serial> shell cat /proc/net/dev` | 1s (backoff to 5s) | Interface bytes / rates |
| `adb -s <serial> shell dumpsys netstats detail` | On first network poll, then every 10s or when a new iface appears | Map iface → WiFi/Mobile/Ethernet |
| `adb -s <serial> shell pm list packages -U` | 2s (backoff to 5s on error) | UID → package for per-app traffic |
| `adb -s <serial> shell dumpsys netstats --uid` | 2s (same poll and backoff as above) | Per-app traffic |
| `adb -s <serial> shell pm list packages` | On select, then 60s after each scan completes | App list for storage sizes |
| `adb -s <serial> shell sh -c '<batch>'` | Batches of 20 packages during that scan | Per-app storage stats |

The `<batch>` argument to `sh -c` is the below command per package, joined with `; `

```sh
pkg='com.example.app'; printf '@PKG@%s\n' "$pkg"; cmd package get-package-storage-stats "$pkg" 2>/dev/null || true
```

Nothing is written to the device.

Closing the window kills the `adb logcat` child and signals pollers to stop. An `adb` command already in flight is not killed; it finishes, then the process exits. The app does not stop the adb server.

## License

Copyright (c) 2026 Gunnar Karlsson. Licensed under the [MIT License](LICENSE).

The UI uses JetBrains Mono Nerd Font (SIL Open Font License) from `crates/android-terminal/assets/fonts`.
