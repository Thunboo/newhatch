use std::{fmt, net::IpAddr, str::FromStr};

use serde::{Deserialize, Serialize};

pub use newhatch_protocol::{Direction, Endpoint, FlowKey, Source};

#[derive(Clone, Debug, Deserialize)]
pub struct SourceInput {
    pub name: String,
    pub port: u16,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i64)]
pub enum SessionProtocol {
    RawTcp = 0,
    Http = 1,
    WebSocket = 2,
}

impl SessionProtocol {
    pub fn from_db(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::RawTcp),
            1 => Some(Self::Http),
            2 => Some(Self::WebSocket),
            _ => None,
        }
    }
}

impl fmt::Display for SessionProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RawTcp => "raw_tcp",
            Self::Http => "http",
            Self::WebSocket => "websocket",
        })
    }
}

impl FromStr for SessionProtocol {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "raw_tcp" => Ok(Self::RawTcp),
            "http" => Ok(Self::Http),
            "websocket" => Ok(Self::WebSocket),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i64)]
pub enum FlagDirection {
    None = 0,
    C2s = 1,
    S2c = 2,
    Both = 3,
}

impl FlagDirection {
    pub fn from_counts(c2s: usize, s2c: usize) -> Self {
        match (c2s > 0, s2c > 0) {
            (false, false) => Self::None,
            (true, false) => Self::C2s,
            (false, true) => Self::S2c,
            (true, true) => Self::Both,
        }
    }

    pub fn from_db(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::C2s),
            2 => Some(Self::S2c),
            3 => Some(Self::Both),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HttpMetadata {
    pub method: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub status: Option<u16>,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CompletedSession {
    pub source_id: i64,
    pub started_at: i64,
    pub ended_at: i64,
    pub client_ip: IpAddr,
    pub client_port: u16,
    pub server_ip: IpAddr,
    pub server_port: u16,
    pub protocol: SessionProtocol,
    pub c2s: Vec<u8>,
    pub s2c: Vec<u8>,
    pub flag_direction: FlagDirection,
    pub flag_count: usize,
    pub incomplete: bool,
    pub http: HttpMetadata,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub source_id: i64,
    pub source_name: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub client_ip: IpAddr,
    pub client_port: u16,
    pub server_ip: IpAddr,
    pub server_port: u16,
    pub protocol: SessionProtocol,
    pub bytes_c2s: u64,
    pub bytes_s2c: u64,
    pub contains_flag: bool,
    pub flag_direction: FlagDirection,
    pub flag_count: u64,
    pub suricata_alerts: u64,
    pub incomplete: bool,
    pub http: HttpMetadata,
}

#[derive(Clone, Debug)]
pub struct StoredSession {
    pub summary: SessionSummary,
    pub segment_id: i64,
    pub segment_filename: String,
    pub segment_offset: u64,
    pub record_length: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionPayload {
    pub c2s: Vec<u8>,
    pub s2c: Vec<u8>,
}
