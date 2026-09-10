use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

/// Noise that changes between copies of the same bug (hex, paths, numbers).
static NOISE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
            0x[0-9a-fA-F]+
          | /[^\s]+
          | \b\d+\b
        ",
    )
    .expect("valid noise regex")
});

/// Secrets and real-world identifiers (MAC, Bearer, JWT-like, email, assignments, IPv4, long digits).
static SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
            (?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}
          | Bearer\s+\S+
          | eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+
          | [A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}
          | (?i:\b(?:password|passwd|pwd|token|api[_-]?key|authorization)\s*[=:]\s*\S+)
          | \b(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\b
          | \b\d{10,15}\b
        ",
    )
    .expect("valid secret regex")
});

const DEVICE_LABEL_HASH_LEN: usize = 8;
const FINGERPRINT_HEX_LEN: usize = 16;

/// Generates a stable, non-reversible device label from model and adb serial.
///
/// Format: `{sanitized_model}:{sha256(serial) hex truncated to DEVICE_LABEL_HASH_LEN}`.
///
/// - `model` — product model from adb (non-alphanumeric chars become `_`)
/// - `serial` — adb device serial (hashed; not included raw in the label)
pub fn generate_device_label(model: &str, serial: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serial.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    let model: String = model
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{model}:{}", &hex[..DEVICE_LABEL_HASH_LEN])
}

/// Generates a hex fingerprint of length `FINGERPRINT_HEX_LEN` from a logcat tag and a noise-stripped message.
///
/// Hashes `tag` plus the message after replacing volatile noise (numbers, paths, hex) with `#`.
///
/// - `tag` — logcat tag (included in the hash)
/// - `message` — logcat message body
pub fn generate_fingerprint(tag: &str, message: &str) -> String {
    let collapsed = NOISE_RE.replace_all(message, "#");
    let mut hasher = Sha256::new();
    hasher.update(tag.as_bytes());
    hasher.update(b"|");
    hasher.update(collapsed.as_bytes());
    format!("{:x}", hasher.finalize())[..FINGERPRINT_HEX_LEN].to_string()
}

/// Generates a copy of `message` with secrets and real-world identifiers replaced by `#`.
///
/// Replaces MACs, Bearer tokens, JWT-like blobs, emails, `password=`/`token=`/`api_key=`/
/// `authorization:` style assignments, IPv4 addresses, and 10–15 digit runs. Leaves paths and
/// short numbers unchanged.
pub fn redact(message: &str) -> String {
    SECRET_RE.replace_all(message, "#").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_numbers_and_paths() {
        let left = generate_fingerprint("OkHttp", "failed 3 times at /data/app/foo");
        let right = generate_fingerprint("OkHttp", "failed 9 times at /data/app/bar");
        assert_eq!(left, right);
    }

    #[test]
    fn different_tags_differ() {
        let left = generate_fingerprint("OkHttp", "boom");
        let right = generate_fingerprint("System", "boom");
        assert_ne!(left, right);
    }

    #[test]
    fn redact_email_bearer_and_mac() {
        let text = redact("user a@b.com Bearer abc.def aa:bb:cc:dd:ee:ff");
        assert!(!text.contains("a@b.com"));
        assert!(!text.contains("abc.def"));
        assert!(!text.contains("aa:bb:cc:dd:ee:ff"));
        assert!(text.contains('#'));
    }

    #[test]
    fn redact_keeps_status_codes_and_paths() {
        let text = redact("HTTP 500 from /data/user/0/com.app/cache");
        assert!(text.contains("500"));
        assert!(text.contains("/data/user/0/com.app/cache"));
    }

    #[test]
    fn redact_assignment_secrets() {
        let text = redact(
            "password=s3cret token=abc123 api_key=key-99 api-key=key-88 authorization: Bearer.xyz",
        );
        assert!(!text.contains("s3cret"));
        assert!(!text.contains("abc123"));
        assert!(!text.contains("key-99"));
        assert!(!text.contains("key-88"));
        assert!(!text.contains("Bearer.xyz"));
        assert!(text.contains('#'));
    }

    #[test]
    fn redact_ipv4() {
        let text = redact("connect failed to 10.0.0.1 port 443");
        assert!(!text.contains("10.0.0.1"));
        assert!(text.contains("port 443"));
        assert!(text.contains('#'));
    }

    #[test]
    fn redact_long_digit_runs() {
        let text = redact("call 5551234567 or pid 12345 code 500");
        assert!(!text.contains("5551234567"));
        assert!(text.contains("12345"));
        assert!(text.contains("500"));
        assert!(text.contains('#'));
    }

    #[test]
    fn generate_device_label_hides_serial() {
        let label = generate_device_label("Pixel 8", "emulator-5554");
        assert!(label.starts_with("Pixel_8:"));
        assert!(!label.contains("emulator-5554"));
        assert_eq!(label, generate_device_label("Pixel 8", "emulator-5554"));
        assert_ne!(label, generate_device_label("Pixel 8", "emulator-5556"));
    }
}
