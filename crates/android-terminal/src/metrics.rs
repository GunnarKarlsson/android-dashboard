use std::collections::HashMap;

use adb_client::{MemoryStats, NetworkStats, ProtocolStats, StorageBreakdown, StorageOverview};

#[derive(Default)]
pub struct AppStorageState {
    pub packages: Vec<String>,
    pub sizes: HashMap<String, u64>,
    pub scanning: bool,
    pub error: Option<String>,
}

impl AppStorageState {
    pub fn set_packages(&mut self, packages: Vec<String>) {
        self.packages = packages;
        self.sizes.clear();
        self.scanning = true;
    }

    pub fn merge_packages(&mut self, packages: Vec<String>) {
        self.packages = packages;
        self.sizes
            .retain(|package, _| self.packages.iter().any(|pkg| pkg == package));
        self.scanning = true;
    }

    pub fn set_size(&mut self, package: &str, bytes: u64) {
        self.sizes.insert(package.to_string(), bytes);
    }

    pub fn sorted_rows(&self) -> Vec<(&str, Option<u64>)> {
        let mut rows: Vec<(&str, Option<u64>)> = self
            .packages
            .iter()
            .map(|pkg| (pkg.as_str(), self.sizes.get(pkg).copied()))
            .collect();
        rows.sort_by(|a, b| match (a.1, b.1) {
            (Some(left), Some(right)) => right.cmp(&left),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(b.0),
        });
        rows
    }
}

pub struct MetricStore {
    pub ram_memory: Option<MemoryStats>,
    pub ram_error: Option<String>,
    pub storage_gauge: Option<StorageOverview>,
    pub storage_gauge_error: Option<String>,
    pub storage_breakdown: Option<StorageBreakdown>,
    pub storage_breakdown_error: Option<String>,
    pub network_stats: Option<NetworkStats>,
    pub network_error: Option<String>,
    pub protocol_stats: Option<ProtocolStats>,
    pub protocol_error: Option<String>,
    pub app_storage: AppStorageState,
}

impl Default for MetricStore {
    fn default() -> Self {
        Self {
            ram_memory: None,
            ram_error: None,
            storage_gauge: None,
            storage_gauge_error: None,
            storage_breakdown: None,
            storage_breakdown_error: None,
            network_stats: None,
            network_error: None,
            protocol_stats: None,
            protocol_error: None,
            app_storage: AppStorageState::default(),
        }
    }
}
