use std::{
    env,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Config {
    pub capture_interface: String,
    pub listen_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub flag_regex: String,
    pub segment_duration: Duration,
    pub segment_retention_count: usize,
    pub flow_workers: usize,
    pub packet_queue_capacity: usize,
    pub storage_queue_capacity: usize,
    pub flow_idle_timeout: Duration,
    pub max_active_flows_per_worker: usize,
    pub max_stream_bytes: usize,
    pub rule_highlighting_enabled: bool,
    pub chain_query_limits: ChainQueryLimits,
    pub analyzer_mode: AnalyzerMode,
    pub collector_listen_addr: SocketAddr,
    pub collector_allowed_ips: Vec<IpAddr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalyzerMode {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug)]
pub struct ChainQueryLimits {
    pub timeout: Duration,
    pub max_vm_steps: u64,
}

impl Default for ChainQueryLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            max_vm_steps: 250_000_000,
        }
    }
}

impl ChainQueryLimits {
    fn parse(timeout: &str, max_vm_steps: &str) -> Result<Self> {
        let limits = Self {
            timeout: parse_duration(timeout).context("invalid CHAIN_QUERY_TIMEOUT")?,
            max_vm_steps: max_vm_steps
                .parse()
                .context("invalid CHAIN_QUERY_MAX_VM_STEPS")?,
        };
        limits.deadline()?;
        Ok(limits)
    }

    pub(crate) fn deadline(self) -> Result<Instant> {
        if self.timeout.is_zero() {
            bail!("CHAIN_QUERY_TIMEOUT must be greater than zero");
        }
        if self.max_vm_steps == 0 {
            bail!("CHAIN_QUERY_MAX_VM_STEPS must be greater than zero");
        }
        Instant::now()
            .checked_add(self.timeout)
            .context("CHAIN_QUERY_TIMEOUT is too large")
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let capture_interface = env_string("CAPTURE_INTERFACE", "eth0");
        if capture_interface.trim().is_empty() {
            bail!("CAPTURE_INTERFACE must not be empty");
        }

        let listen_addr: SocketAddr = env_string("LISTEN_ADDR", "127.0.0.1:3000")
            .parse()
            .context("invalid LISTEN_ADDR")?;
        if !listen_addr.ip().is_loopback() {
            bail!("LISTEN_ADDR must be a loopback address");
        }
        let data_dir = PathBuf::from(env_string("DATA_DIR", "./data"));
        let flag_regex = env_string("FLAG_REGEX", r"FLAG\{[^}\r\n]+\}");
        let compiled_flag_regex =
            regex::bytes::Regex::new(&flag_regex).context("invalid FLAG_REGEX")?;
        if compiled_flag_regex.is_match(b"") {
            bail!("FLAG_REGEX must not match an empty string");
        }

        let segment_duration = parse_duration_env("SEGMENT_DURATION", "30m")?;
        let segment_retention_count = parse_env("SEGMENT_RETENTION_COUNT", 3usize)?;
        if segment_retention_count == 0 {
            bail!("SEGMENT_RETENTION_COUNT must be greater than zero");
        }

        // Basically - nproc
        let available_workers = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        // If env-var is available - take its value, other - min=1 and nproc with max of 8 logical cores
        let flow_workers = parse_env("FLOW_WORKERS", available_workers.clamp(1, 8))?;
        if flow_workers == 0 {
            bail!("FLOW_WORKERS must be greater than zero");
        }

        let analyzer_mode = match env_string("ANALYZER", "local").as_str() {
            "local" => AnalyzerMode::Local,
            "remote" => AnalyzerMode::Remote,
            _ => bail!("ANALYZER must be local or remote"),
        };
        let collector_listen_addr = resolve_socket_addr(
            "LISTEN_CONNSTR",
            &env_string("LISTEN_CONNSTR", "0.0.0.0:39090"),
        )?;
        let collector_allowed_ips = parse_ip_list("ALLOWED_COLLECTORS")?;

        Ok(Self {
            capture_interface,
            listen_addr,
            data_dir,
            flag_regex,
            segment_duration,
            segment_retention_count,
            flow_workers,
            packet_queue_capacity: parse_env("PACKET_QUEUE_CAPACITY", 8_192usize)?,
            storage_queue_capacity: parse_env("STORAGE_QUEUE_CAPACITY", 2_048usize)?,
            flow_idle_timeout: parse_duration_env("FLOW_IDLE_TIMEOUT", "15s")?,
            max_active_flows_per_worker: parse_env("MAX_ACTIVE_FLOWS_PER_WORKER", 16_384usize)?,
            max_stream_bytes: parse_size_env("MAX_STREAM_BYTES", "16MiB")?,
            rule_highlighting_enabled: parse_env("RULE_HIGHLIGHTING_ENABLED", true)?,
            chain_query_limits: ChainQueryLimits::parse(
                &env_string("CHAIN_QUERY_TIMEOUT", "10s"),
                &env_string("CHAIN_QUERY_MAX_VM_STEPS", "250000000"),
            )?,
            analyzer_mode,
            collector_listen_addr,
            collector_allowed_ips,
        })
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.data_dir.join("index.sqlite")
    }
}

fn parse_ip_list(name: &str) -> Result<Vec<IpAddr>> {
    let value = env::var(name).unwrap_or_default();
    resolve_ip_list(name, &value)
}

fn resolve_socket_addr(name: &str, value: &str) -> Result<SocketAddr> {
    value
        .to_socket_addrs()
        .with_context(|| format!("invalid or unresolvable {name}"))?
        .next()
        .with_context(|| format!("{name} resolved no addresses"))
}

fn resolve_ip_list(name: &str, value: &str) -> Result<Vec<IpAddr>> {
    let mut addresses = Vec::new();
    for item in value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if let Ok(address) = item.parse::<IpAddr>() {
            if !addresses.contains(&address) {
                addresses.push(address);
            }
            continue;
        }
        let resolved = (item, 0)
            .to_socket_addrs()
            .with_context(|| format!("invalid or unresolvable host in {name}: {item}"))?;
        let mut found = false;
        for address in resolved {
            found = true;
            if !addresses.contains(&address.ip()) {
                addresses.push(address.ip());
            }
        }
        if !found {
            bail!("host in {name} resolved no addresses: {item}");
        }
    }
    Ok(addresses)
}

fn env_string(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn parse_env<T>(name: &str, default: T) -> Result<T>
where
    T: FromStr + std::fmt::Display,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("invalid {name}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("failed to read {name}")),
    }
}

fn parse_duration_env(name: &str, default: &str) -> Result<Duration> {
    let raw = env_string(name, default);
    parse_duration(&raw).with_context(|| format!("invalid {name}"))
}

fn parse_duration(raw: &str) -> Result<Duration> {
    let split = raw
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(raw.len());
    let (value, unit) = raw.split_at(split);
    let value: u64 = value
        .parse()
        .context("duration must start with an integer")?;
    match unit {
        "ms" => Ok(Duration::from_millis(value)),
        "s" => Ok(Duration::from_secs(value)),
        "m" => Ok(Duration::from_secs(value.saturating_mul(60))),
        "h" => Ok(Duration::from_secs(value.saturating_mul(3_600))),
        _ => bail!("supported duration units are ms, s, m and h"),
    }
}

fn parse_size_env(name: &str, default: &str) -> Result<usize> {
    let raw = env_string(name, default);
    let split = raw
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(raw.len());
    let (value, unit) = raw.split_at(split);
    let value: usize = value.parse().with_context(|| format!("invalid {name}"))?;
    let multiplier = match unit {
        "B" | "" => 1,
        "KiB" => 1_024,
        "MiB" => 1_024 * 1_024,
        "GiB" => 1_024 * 1_024 * 1_024,
        _ => bail!("{name} supports B, KiB, MiB and GiB"),
    };
    value
        .checked_mul(multiplier)
        .with_context(|| format!("{name} is too large"))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_duration, parse_env, parse_size_env, resolve_ip_list, resolve_socket_addr,
        ChainQueryLimits,
    };
    use std::time::Duration;

    #[test]
    fn parses_supported_durations() {
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1_800));
    }

    #[test]
    fn rejects_duration_without_unit() {
        assert!(parse_duration("30").is_err());
    }

    #[test]
    fn chain_query_limits_have_raised_defaults_and_accept_overrides() {
        let defaults = ChainQueryLimits::default();
        assert_eq!(defaults.timeout, Duration::from_secs(10));
        assert_eq!(defaults.max_vm_steps, 250_000_000);
        let limits = ChainQueryLimits::parse("1500ms", "500000000").unwrap();
        assert_eq!(limits.timeout, Duration::from_millis(1_500));
        assert_eq!(limits.max_vm_steps, 500_000_000);
    }

    #[test]
    fn chain_query_limits_reject_disabled_and_invalid_budgets() {
        for timeout in ["0s", "-1s", "10", "forever", "18446744073709551615h"] {
            assert!(ChainQueryLimits::parse(timeout, "250000000")
                .unwrap_err()
                .to_string()
                .contains("CHAIN_QUERY_TIMEOUT"));
        }
        for steps in ["0", "-1", "", "unlimited", "18446744073709551616"] {
            assert!(ChainQueryLimits::parse("10s", steps)
                .unwrap_err()
                .to_string()
                .contains("CHAIN_QUERY_MAX_VM_STEPS"));
        }
    }

    #[test]
    fn default_size_is_valid() {
        std::env::remove_var("NEWHATCH_TEST_SIZE");
        assert_eq!(
            parse_size_env("NEWHATCH_TEST_SIZE", "16MiB").unwrap(),
            16 * 1_024 * 1_024
        );
    }

    #[test]
    fn rule_highlighting_setting_defaults_on_and_rejects_invalid_values() {
        const KEY: &str = "NEWHATCH_TEST_RULE_HIGHLIGHTING";
        std::env::remove_var(KEY);
        assert!(parse_env(KEY, true).unwrap());
        std::env::set_var(KEY, "false");
        assert!(!parse_env(KEY, true).unwrap());
        std::env::set_var(KEY, "true");
        assert!(parse_env(KEY, true).unwrap());
        for value in ["", "0", "1", "yes", "False"] {
            std::env::set_var(KEY, value);
            assert!(parse_env(KEY, true).unwrap_err().to_string().contains(KEY));
        }
        std::env::remove_var(KEY);
    }

    #[test]
    fn resolves_fqdn_endpoints_and_allowlist_entries() {
        assert_eq!(
            resolve_socket_addr("TEST_ENDPOINT", "localhost:39090")
                .unwrap()
                .port(),
            39090
        );
        let addresses = resolve_ip_list("TEST_HOSTS", "127.0.0.1,localhost").unwrap();
        assert!(addresses.contains(&"127.0.0.1".parse().unwrap()));
        assert!(addresses.iter().all(|address| address.is_loopback()));
    }
}
