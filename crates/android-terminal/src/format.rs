pub(crate) fn truncate_package_name(name: &str) -> String {
    const MAX_LEN: usize = 42;
    if name.len() <= MAX_LEN {
        return name.to_string();
    }
    format!("{}...", &name[..MAX_LEN - 3])
}

pub(crate) fn format_bytes_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    }
}

pub(crate) fn format_rate_mb(rx_bps: f64, tx_bps: f64) -> String {
    format!(
        "↓ {}  ↑ {}",
        format_throughput(rx_bps),
        format_throughput(tx_bps)
    )
}

fn format_throughput(bps: f64) -> String {
    if bps < 0.0 {
        return "—".to_string();
    }
    if bps >= 1_048_576.0 {
        format!("{:.2} MB/s", bps / 1_048_576.0)
    } else if bps >= 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else {
        format!("{bps:.0} B/s")
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn format_gb_from_kb(kb: u64) -> String {
    format!("{:.1} GB", kb as f64 / 1_048_576.0)
}

pub(crate) fn format_gb_from_bytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}
