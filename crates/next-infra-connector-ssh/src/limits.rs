pub const MAX_PROBES_PER_BATCH: usize = 6;
// Mirrors the Host-side connect timeout validation range (5..=14 seconds).
pub const MAX_CONNECT_TIMEOUT_SECS: u8 = 14;
pub const MAX_PROBE_WALL_TIME_SECS: u64 = 20;
pub const MAX_BATCH_WALL_TIME_SECS: u64 = 90;
pub const MAX_PROBE_STDERR_BYTES: usize = 32 * 1024;
pub const MAX_BATCH_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
