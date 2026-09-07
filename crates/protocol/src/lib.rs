use std::{io, net::IpAddr};

use prost::Message;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const WIRE_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
pub type CollectorId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub port: u16,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    C2s,
    S2c,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Endpoint {
    pub ip: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FlowKey {
    pub client: Endpoint,
    pub server: Endpoint,
}

#[derive(Clone, Debug)]
pub struct TcpPacket {
    pub timestamp_micros: i64,
    pub src: Endpoint,
    pub dst: Endpoint,
    pub sequence: u32,
    pub syn: bool,
    pub fin: bool,
    pub rst: bool,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ClassifiedPacket {
    pub source_id: i64,
    pub direction: Direction,
    pub flow_key: FlowKey,
    pub packet: TcpPacket,
}

#[derive(Clone, PartialEq, Message)]
pub struct Envelope {
    #[prost(uint32, tag = "1")]
    pub version: u32,
    #[prost(oneof = "envelope::Body", tags = "2, 3, 4, 5, 6, 7, 8")]
    pub body: Option<envelope::Body>,
}

pub mod envelope {
    use super::{
        CollectorHello, ErrorStatus, Heartbeat, HelloAck, PacketMessage, SetSources, Stats,
    };
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Body {
        #[prost(message, tag = "2")]
        CollectorHello(CollectorHello),
        #[prost(message, tag = "3")]
        HelloAck(HelloAck),
        #[prost(message, tag = "4")]
        Packet(PacketMessage),
        #[prost(message, tag = "5")]
        SetSources(SetSources),
        #[prost(message, tag = "6")]
        Stats(Stats),
        #[prost(message, tag = "7")]
        Heartbeat(Heartbeat),
        #[prost(message, tag = "8")]
        ErrorStatus(ErrorStatus),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct CollectorHello {
    #[prost(string, tag = "1")]
    pub collector_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct HelloAck {
    #[prost(int64, tag = "1")]
    pub server_time_micros: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct PacketMessage {
    #[prost(int64, tag = "1")]
    pub timestamp_micros: i64,
    #[prost(int64, tag = "2")]
    pub source_id: i64,
    #[prost(enumeration = "WireDirection", tag = "3")]
    pub direction: i32,
    #[prost(bytes = "vec", tag = "4")]
    pub client_ip: Vec<u8>,
    #[prost(uint32, tag = "5")]
    pub client_port: u32,
    #[prost(bytes = "vec", tag = "6")]
    pub server_ip: Vec<u8>,
    #[prost(uint32, tag = "7")]
    pub server_port: u32,
    #[prost(bytes = "vec", tag = "8")]
    pub src_ip: Vec<u8>,
    #[prost(uint32, tag = "9")]
    pub src_port: u32,
    #[prost(bytes = "vec", tag = "10")]
    pub dst_ip: Vec<u8>,
    #[prost(uint32, tag = "11")]
    pub dst_port: u32,
    #[prost(uint32, tag = "12")]
    pub sequence: u32,
    #[prost(bool, tag = "13")]
    pub syn: bool,
    #[prost(bool, tag = "14")]
    pub fin: bool,
    #[prost(bool, tag = "15")]
    pub rst: bool,
    #[prost(bytes = "vec", tag = "16")]
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum WireDirection {
    C2s = 0,
    S2c = 1,
}

#[derive(Clone, PartialEq, Message)]
pub struct SetSources {
    #[prost(uint64, tag = "1")]
    pub revision: u64,
    #[prost(message, repeated, tag = "2")]
    pub sources: Vec<WireSource>,
}

#[derive(Clone, PartialEq, Message)]
pub struct WireSource {
    #[prost(int64, tag = "1")]
    pub id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(uint32, tag = "3")]
    pub port: u32,
    #[prost(bool, tag = "4")]
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct Stats {
    #[prost(uint64, tag = "1")]
    pub captured_packets: u64,
    #[prost(uint64, tag = "2")]
    pub sent_packets: u64,
    #[prost(uint64, tag = "3")]
    pub dropped_packets: u64,
    #[prost(uint64, tag = "4")]
    pub queue_depth: u64,
    #[prost(uint64, tag = "5")]
    pub reconnect_count: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Heartbeat {
    #[prost(int64, tag = "1")]
    pub timestamp_micros: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ErrorStatus {
    #[prost(string, tag = "1")]
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid protobuf frame: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("unsupported wire protocol version {0}")]
    Version(u32),
    #[error("frame size {0} exceeds limit")]
    FrameTooLarge(usize),
    #[error("invalid packet field: {0}")]
    InvalidPacket(&'static str),
}

impl Envelope {
    pub fn new(body: envelope::Body) -> Self {
        Self {
            version: WIRE_VERSION,
            body: Some(body),
        }
    }
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    envelope: &Envelope,
) -> Result<(), ProtocolError> {
    let length = envelope.encoded_len();
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(length));
    }
    writer.write_u32(length as u32).await?;
    let mut bytes = Vec::with_capacity(length);
    envelope
        .encode(&mut bytes)
        .expect("Vec encoding cannot fail");
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Envelope, ProtocolError> {
    let length = reader.read_u32().await? as usize;
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(length));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    let envelope = Envelope::decode(bytes.as_slice())?;
    if envelope.version != WIRE_VERSION {
        return Err(ProtocolError::Version(envelope.version));
    }
    Ok(envelope)
}

impl From<&Source> for WireSource {
    fn from(source: &Source) -> Self {
        Self {
            id: source.id,
            name: source.name.clone(),
            port: u32::from(source.port),
            enabled: source.enabled,
        }
    }
}

impl TryFrom<WireSource> for Source {
    type Error = ProtocolError;
    fn try_from(source: WireSource) -> Result<Self, Self::Error> {
        Ok(Self {
            id: source.id,
            name: source.name,
            port: checked_port(source.port)?,
            enabled: source.enabled,
        })
    }
}

impl From<&ClassifiedPacket> for PacketMessage {
    fn from(value: &ClassifiedPacket) -> Self {
        Self {
            timestamp_micros: value.packet.timestamp_micros,
            source_id: value.source_id,
            direction: match value.direction {
                Direction::C2s => WireDirection::C2s,
                Direction::S2c => WireDirection::S2c,
            } as i32,
            client_ip: ip_bytes(value.flow_key.client.ip),
            client_port: u32::from(value.flow_key.client.port),
            server_ip: ip_bytes(value.flow_key.server.ip),
            server_port: u32::from(value.flow_key.server.port),
            src_ip: ip_bytes(value.packet.src.ip),
            src_port: u32::from(value.packet.src.port),
            dst_ip: ip_bytes(value.packet.dst.ip),
            dst_port: u32::from(value.packet.dst.port),
            sequence: value.packet.sequence,
            syn: value.packet.syn,
            fin: value.packet.fin,
            rst: value.packet.rst,
            payload: value.packet.payload.clone(),
        }
    }
}

impl TryFrom<PacketMessage> for ClassifiedPacket {
    type Error = ProtocolError;
    fn try_from(value: PacketMessage) -> Result<Self, Self::Error> {
        let endpoint = |ip, port| -> Result<Endpoint, ProtocolError> {
            Ok(Endpoint {
                ip: parse_ip(ip)?,
                port: checked_port(port)?,
            })
        };
        Ok(Self {
            source_id: value.source_id,
            direction: match WireDirection::try_from(value.direction)
                .map_err(|_| ProtocolError::InvalidPacket("direction"))?
            {
                WireDirection::C2s => Direction::C2s,
                WireDirection::S2c => Direction::S2c,
            },
            flow_key: FlowKey {
                client: endpoint(value.client_ip, value.client_port)?,
                server: endpoint(value.server_ip, value.server_port)?,
            },
            packet: TcpPacket {
                timestamp_micros: value.timestamp_micros,
                src: endpoint(value.src_ip, value.src_port)?,
                dst: endpoint(value.dst_ip, value.dst_port)?,
                sequence: value.sequence,
                syn: value.syn,
                fin: value.fin,
                rst: value.rst,
                payload: value.payload,
            },
        })
    }
}

fn checked_port(port: u32) -> Result<u16, ProtocolError> {
    port.try_into()
        .map_err(|_| ProtocolError::InvalidPacket("TCP port"))
}
fn ip_bytes(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    }
}
fn parse_ip(bytes: Vec<u8>) -> Result<IpAddr, ProtocolError> {
    match bytes.len() {
        4 => Ok(IpAddr::from(
            <[u8; 4]>::try_from(bytes).expect("length checked"),
        )),
        16 => Ok(IpAddr::from(
            <[u8; 16]>::try_from(bytes).expect("length checked"),
        )),
        _ => Err(ProtocolError::InvalidPacket("IP address")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn packet_and_control_messages_round_trip() {
        let packet = ClassifiedPacket {
            source_id: 7,
            direction: Direction::C2s,
            flow_key: FlowKey {
                client: Endpoint {
                    ip: "10.0.0.2".parse().unwrap(),
                    port: 50123,
                },
                server: Endpoint {
                    ip: "10.0.0.1".parse().unwrap(),
                    port: 8080,
                },
            },
            packet: TcpPacket {
                timestamp_micros: 42,
                src: Endpoint {
                    ip: "10.0.0.2".parse().unwrap(),
                    port: 50123,
                },
                dst: Endpoint {
                    ip: "10.0.0.1".parse().unwrap(),
                    port: 8080,
                },
                sequence: 9,
                syn: true,
                fin: false,
                rst: false,
                payload: b"hello".to_vec(),
            },
        };
        let messages = vec![
            Envelope::new(envelope::Body::CollectorHello(CollectorHello {
                collector_id: "vulnbox-1".into(),
            })),
            Envelope::new(envelope::Body::Packet(PacketMessage::from(&packet))),
            Envelope::new(envelope::Body::SetSources(SetSources {
                revision: 2,
                sources: vec![WireSource::from(&Source {
                    id: 7,
                    name: "web".into(),
                    port: 8080,
                    enabled: true,
                })],
            })),
            Envelope::new(envelope::Body::Stats(Stats {
                captured_packets: 1,
                sent_packets: 1,
                dropped_packets: 0,
                queue_depth: 0,
                reconnect_count: 0,
            })),
        ];
        for message in messages {
            let (mut client, mut server) = tokio::io::duplex(4096);
            write_frame(&mut client, &message).await.unwrap();
            assert_eq!(read_frame(&mut server).await.unwrap(), message);
        }
        let decoded = ClassifiedPacket::try_from(PacketMessage::from(&packet)).unwrap();
        assert_eq!(decoded.flow_key, packet.flow_key);
        assert_eq!(decoded.packet.payload, b"hello");
    }
}
