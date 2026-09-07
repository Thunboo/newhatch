use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::domain::{Direction, Endpoint, FlowKey, Source};

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

pub fn parse_ethernet_tcp(frame: &[u8], timestamp_micros: i64) -> Option<TcpPacket> {
    if frame.len() < 14 {
        return None;
    }
    let mut network_offset = 14usize;
    let mut ether_type = u16::from_be_bytes([frame[12], frame[13]]);
    if ether_type == 0x8100 || ether_type == 0x88a8 {
        if frame.len() < 18 {
            return None;
        }
        ether_type = u16::from_be_bytes([frame[16], frame[17]]);
        network_offset = 18;
    }

    let (src_ip, dst_ip, tcp_offset, network_end) = match ether_type {
        0x0800 => parse_ipv4(frame, network_offset)?,
        0x86dd => parse_ipv6(frame, network_offset)?,
        _ => return None,
    };
    parse_tcp(
        frame,
        timestamp_micros,
        src_ip,
        dst_ip,
        tcp_offset,
        network_end,
    )
}

pub fn parse_ip_tcp(packet: &[u8], timestamp_micros: i64) -> Option<TcpPacket> {
    let (src_ip, dst_ip, tcp_offset, network_end) = match packet.first()? >> 4 {
        4 => parse_ipv4(packet, 0)?,
        6 => parse_ipv6(packet, 0)?,
        _ => return None,
    };
    parse_tcp(
        packet,
        timestamp_micros,
        src_ip,
        dst_ip,
        tcp_offset,
        network_end,
    )
}

pub fn classify(packet: TcpPacket, sources: &[Source]) -> Option<ClassifiedPacket> {
    let (source_id, direction, client, server) = if let Some(source) = sources
        .iter()
        .find(|source| source.enabled && source.port == packet.dst.port)
    {
        (
            source.id,
            Direction::C2s,
            packet.src.clone(),
            packet.dst.clone(),
        )
    } else if let Some(source) = sources
        .iter()
        .find(|source| source.enabled && source.port == packet.src.port)
    {
        (
            source.id,
            Direction::S2c,
            packet.dst.clone(),
            packet.src.clone(),
        )
    } else {
        return None;
    };

    Some(ClassifiedPacket {
        source_id,
        direction,
        flow_key: FlowKey { client, server },
        packet,
    })
}

fn parse_ipv4(frame: &[u8], offset: usize) -> Option<(IpAddr, IpAddr, usize, usize)> {
    if frame.len() < offset + 20 || frame[offset] >> 4 != 4 || frame[offset + 9] != 6 {
        return None;
    }
    let header_length = usize::from(frame[offset] & 0x0f) * 4;
    if header_length < 20 || frame.len() < offset + header_length {
        return None;
    }
    let total_length = usize::from(u16::from_be_bytes([frame[offset + 2], frame[offset + 3]]));
    if total_length < header_length {
        return None;
    }
    let network_end = frame.len().min(offset.saturating_add(total_length));
    let src = IpAddr::V4(Ipv4Addr::new(
        frame[offset + 12],
        frame[offset + 13],
        frame[offset + 14],
        frame[offset + 15],
    ));
    let dst = IpAddr::V4(Ipv4Addr::new(
        frame[offset + 16],
        frame[offset + 17],
        frame[offset + 18],
        frame[offset + 19],
    ));
    Some((src, dst, offset + header_length, network_end))
}

fn parse_ipv6(frame: &[u8], offset: usize) -> Option<(IpAddr, IpAddr, usize, usize)> {
    if frame.len() < offset + 40 || frame[offset] >> 4 != 6 || frame[offset + 6] != 6 {
        // IPv6 extension-header walking is deliberately deferred.
        return None;
    }
    let payload_length = usize::from(u16::from_be_bytes([frame[offset + 4], frame[offset + 5]]));
    let network_end = frame
        .len()
        .min(offset.saturating_add(40).saturating_add(payload_length));
    let src = IpAddr::V6(Ipv6Addr::from(
        <[u8; 16]>::try_from(&frame[offset + 8..offset + 24]).ok()?,
    ));
    let dst = IpAddr::V6(Ipv6Addr::from(
        <[u8; 16]>::try_from(&frame[offset + 24..offset + 40]).ok()?,
    ));
    Some((src, dst, offset + 40, network_end))
}

fn parse_tcp(
    frame: &[u8],
    timestamp_micros: i64,
    src_ip: IpAddr,
    dst_ip: IpAddr,
    offset: usize,
    network_end: usize,
) -> Option<TcpPacket> {
    if network_end < offset + 20 || frame.len() < offset + 20 {
        return None;
    }
    let header_length = usize::from(frame[offset + 12] >> 4) * 4;
    if header_length < 20 || network_end < offset + header_length {
        return None;
    }
    let flags = frame[offset + 13];
    Some(TcpPacket {
        timestamp_micros,
        src: Endpoint {
            ip: src_ip,
            port: u16::from_be_bytes([frame[offset], frame[offset + 1]]),
        },
        dst: Endpoint {
            ip: dst_ip,
            port: u16::from_be_bytes([frame[offset + 2], frame[offset + 3]]),
        },
        sequence: u32::from_be_bytes([
            frame[offset + 4],
            frame[offset + 5],
            frame[offset + 6],
            frame[offset + 7],
        ]),
        fin: flags & 0x01 != 0,
        syn: flags & 0x02 != 0,
        rst: flags & 0x04 != 0,
        payload: frame[offset + header_length..network_end].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use crate::domain::Source;

    use super::{classify, parse_ethernet_tcp, parse_ip_tcp};

    #[test]
    fn parses_and_classifies_ipv4_tcp() {
        let mut frame = vec![0u8; 14 + 20 + 20 + 3];
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[16..18].copy_from_slice(&(43u16).to_be_bytes());
        frame[23] = 6;
        frame[26..30].copy_from_slice(&[10, 0, 0, 2]);
        frame[30..34].copy_from_slice(&[10, 0, 0, 1]);
        frame[34..36].copy_from_slice(&51_234u16.to_be_bytes());
        frame[36..38].copy_from_slice(&8_080u16.to_be_bytes());
        frame[38..42].copy_from_slice(&100u32.to_be_bytes());
        frame[46] = 5 << 4;
        frame[47] = 0x18;
        frame[54..57].copy_from_slice(b"GET");

        let packet = parse_ethernet_tcp(&frame, 123).unwrap();
        assert_eq!(packet.payload, b"GET");
        assert_eq!(packet.src.ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)));
        let classified = classify(
            packet,
            &[Source {
                id: 1,
                name: "web".into(),
                port: 8_080,
                enabled: true,
            }],
        )
        .unwrap();
        assert_eq!(classified.source_id, 1);
    }

    #[test]
    fn parses_layer_three_ipv4_from_cooked_packet_socket() {
        let mut packet = vec![0u8; 20 + 20 + 3];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(43u16).to_be_bytes());
        packet[9] = 6;
        packet[12..16].copy_from_slice(&[100, 97, 52, 206]);
        packet[16..20].copy_from_slice(&[100, 97, 69, 83]);
        packet[20..22].copy_from_slice(&51_465u16.to_be_bytes());
        packet[22..24].copy_from_slice(&18_080u16.to_be_bytes());
        packet[24..28].copy_from_slice(&100u32.to_be_bytes());
        packet[32] = 5 << 4;
        packet[33] = 0x18;
        packet[40..43].copy_from_slice(b"GET");

        let parsed = parse_ip_tcp(&packet, 123).unwrap();
        assert_eq!(parsed.src.port, 51_465);
        assert_eq!(parsed.dst.port, 18_080);
        assert_eq!(parsed.payload, b"GET");
    }
}
