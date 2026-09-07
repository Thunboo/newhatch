use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    time::Duration,
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
    pub ingress_mode: IngressMode,
    pub collector_listen_addr: SocketAddr,
    pub collector_allowed_ips: Vec<IpAddr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IngressMode {
    Local,
    Receiver,
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

        let ingress_mode = match env_string("PACKET_INGRESS_MODE", "local").as_str() {
            "local" => IngressMode::Local,
            "receiver" => IngressMode::Receiver,
            _ => bail!("PACKET_INGRESS_MODE must be local or receiver"),
        };
        let collector_listen_addr = env_string("COLLECTOR_LISTEN_ADDR", "0.0.0.0:39090")
            .parse()
            .context("invalid COLLECTOR_LISTEN_ADDR")?;
        let collector_allowed_ips = parse_ip_list("COLLECTOR_ALLOWED_IPS")?;
        if ingress_mode == IngressMode::Receiver && collector_allowed_ips.is_empty() {
            bail!("COLLECTOR_ALLOWED_IPS must contain at least one IP in receiver mode");
        }

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
            flow_idle_timeout: parse_duration_env("FLOW_IDLE_TIMEOUT", "30s")?,
            max_active_flows_per_worker: parse_env("MAX_ACTIVE_FLOWS_PER_WORKER", 16_384usize)?,
            max_stream_bytes: parse_size_env("MAX_STREAM_BYTES", "4MiB")?,
            ingress_mode,
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
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse()
                .with_context(|| format!("invalid IP in {name}"))
        })
        .collect()
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
    use super::{parse_duration, parse_size_env};
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
    fn default_size_is_valid() {
        std::env::remove_var("NEWHATCH_TEST_SIZE");
        assert_eq!(
            parse_size_env("NEWHATCH_TEST_SIZE", "4MiB").unwrap(),
            4 * 1_024 * 1_024
        );
    }
}
