use crate::adb::run_adb_for_serial;
use crate::error::AdbError;

/// Memory statistics from `/proc/meminfo` (values in kB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub total_kb: u64,
    pub free_kb: u64,
    pub available_kb: u64,
    pub buffers_kb: u64,
    pub cached_kb: u64,
}

impl MemoryStats {
    pub fn used_kb(&self) -> u64 {
        self.total_kb.saturating_sub(self.available_kb)
    }

    pub fn used_fraction(&self) -> f32 {
        if self.total_kb == 0 {
            0.0
        } else {
            self.used_kb() as f32 / self.total_kb as f32
        }
    }
}

pub fn fetch_memory_stats(serial: &str) -> Result<MemoryStats, AdbError> {
    let meminfo = run_adb_for_serial(serial, &["shell", "cat", "/proc/meminfo"])?;
    parse_meminfo(&String::from_utf8_lossy(&meminfo.stdout))
}

fn parse_meminfo(text: &str) -> Result<MemoryStats, AdbError> {
    let mut total_kb = None;
    let mut free_kb = None;
    let mut available_kb = None;
    let mut buffers_kb = None;
    let mut cached_kb = None;

    for line in text.lines() {
        let (key, value) = match line.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };

        let kb = parse_kb_value(value)?;
        match key.trim() {
            "MemTotal" => total_kb = Some(kb),
            "MemFree" => free_kb = Some(kb),
            "MemAvailable" => available_kb = Some(kb),
            "Buffers" => buffers_kb = Some(kb),
            "Cached" => cached_kb = Some(kb),
            _ => {}
        }
    }

    Ok(MemoryStats {
        total_kb: total_kb.ok_or_else(|| AdbError::ParseFailed("missing MemTotal".into()))?,
        free_kb: free_kb.unwrap_or(0),
        available_kb: available_kb.unwrap_or(0),
        buffers_kb: buffers_kb.unwrap_or(0),
        cached_kb: cached_kb.unwrap_or(0),
    })
}

fn parse_kb_value(raw: &str) -> Result<u64, AdbError> {
    let kb = raw
        .split_whitespace()
        .next()
        .ok_or_else(|| AdbError::ParseFailed(format!("invalid meminfo value: {raw}")))?;

    kb.parse()
        .map_err(|_| AdbError::ParseFailed(format!("invalid meminfo kB value: {raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_meminfo_sample() {
        let sample = r#"MemTotal:        8025332 kB
MemFree:          123456 kB
MemAvailable:    3456789 kB
Buffers:          111111 kB
Cached:           222222 kB
"#;

        let memory = parse_meminfo(sample).unwrap();
        assert_eq!(memory.total_kb, 8_025_332);
        assert_eq!(memory.free_kb, 123_456);
        assert_eq!(memory.available_kb, 3_456_789);
        assert_eq!(memory.buffers_kb, 111_111);
        assert_eq!(memory.cached_kb, 222_222);
        assert_eq!(memory.used_kb(), 8_025_332 - 3_456_789);
    }
}
