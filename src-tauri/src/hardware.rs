//! Pick the polish LLM from RAM and CPU so an 8 GB machine stays light
//! and a 16 GB+ machine gets the stronger model — no settings toggle.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineTier {
    Lower,
    Higher,
}

impl MachineTier {
    pub fn as_id(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Higher => "higher",
        }
    }
}

/// 12 GiB: 8 GB Macs stay below, 16 GB and up sit above (Apple reports a
/// little less than the sticker number).
const HIGHER_RAM_BYTES: u64 = 12 * 1024 * 1024 * 1024;
const HIGHER_CPU_COUNT: usize = 6;

pub fn machine_tier() -> MachineTier {
    classify(physical_memory_bytes(), num_cpus::get())
}

pub fn classify(ram_bytes: Option<u64>, cpu_count: usize) -> MachineTier {
    match ram_bytes {
        Some(ram) if ram >= HIGHER_RAM_BYTES && cpu_count >= HIGHER_CPU_COUNT => {
            MachineTier::Higher
        }
        Some(_) => MachineTier::Lower,
        None if cpu_count >= 8 => MachineTier::Higher,
        None => MachineTier::Lower,
    }
}

pub fn physical_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSProcessInfo;
        Some(NSProcessInfo::processInfo().physicalMemory())
    }
    #[cfg(target_os = "linux")]
    {
        linux_memtotal()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_memtotal() -> Option<u64> {
    let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in raw.lines() {
        let Some(rest) = line.strip_prefix("MemTotal:") else {
            continue;
        };
        let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
        return Some(kb.saturating_mul(1024));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_gig_macbook_is_lower() {
        assert_eq!(
            classify(Some(8 * 1024 * 1024 * 1024), 8),
            MachineTier::Lower
        );
    }

    #[test]
    fn sixteen_gig_machine_is_higher() {
        assert_eq!(
            classify(Some(16 * 1024 * 1024 * 1024), 8),
            MachineTier::Higher
        );
    }

    #[test]
    fn unknown_ram_uses_cpu_count() {
        assert_eq!(classify(None, 4), MachineTier::Lower);
        assert_eq!(classify(None, 8), MachineTier::Higher);
    }
}
