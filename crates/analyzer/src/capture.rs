use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, watch};

use crate::{
    domain::Source,
    flow::{worker_index, IngressPacket},
    storage::Catalog,
};

pub async fn run(
    interface: String,
    catalog: Catalog,
    mut source_revision: watch::Receiver<u64>,
    workers: Vec<mpsc::Sender<IngressPacket>>,
) -> Result<()> {
    if workers.is_empty() {
        anyhow::bail!("capture requires at least one flow worker");
    }

    #[cfg(target_os = "linux")]
    {
        linux::run(interface, catalog, &mut source_revision, workers).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interface, catalog, workers);
        tracing::warn!("live AF_PACKET capture is available only on Linux");
        loop {
            source_revision
                .changed()
                .await
                .context("source revision channel closed")?;
        }
    }
}

async fn load_sources(catalog: Catalog) -> Result<Arc<Vec<Source>>> {
    tokio::task::spawn_blocking(move || catalog.list_sources(true))
        .await
        .context("source loader task panicked")?
        .map(Arc::new)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        ffi::CString,
        io,
        mem::{self, size_of},
        os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    };

    use anyhow::{bail, Context, Result};
    use tokio::{io::unix::AsyncFd, sync::mpsc};

    use crate::{
        flow::IngressPacket,
        packet::{classify, parse_ip_tcp},
        storage::Catalog,
    };

    use super::{load_sources, worker_index};

    const ETH_P_ALL: u16 = 0x0003;
    const IPPROTO_TCP: u32 = 6;

    const BPF_LD: u16 = 0x00;
    const BPF_LDX: u16 = 0x01;
    const BPF_ALU: u16 = 0x04;
    const BPF_JMP: u16 = 0x05;
    const BPF_RET: u16 = 0x06;
    const BPF_H: u16 = 0x08;
    const BPF_B: u16 = 0x10;
    const BPF_ABS: u16 = 0x20;
    const BPF_IND: u16 = 0x40;
    const BPF_MSH: u16 = 0xa0;
    const BPF_JEQ: u16 = 0x10;
    const BPF_AND: u16 = 0x50;
    const BPF_JA: u16 = 0x00;
    const BPF_K: u16 = 0x00;

    pub async fn run(
        interface: String,
        catalog: Catalog,
        source_revision: &mut tokio::sync::watch::Receiver<u64>,
        workers: Vec<mpsc::Sender<IngressPacket>>,
    ) -> Result<()> {
        let mut dropped_packets = 0u64;
        loop {
            let sources = load_sources(catalog.clone()).await?;
            if sources.is_empty() {
                tracing::info!("capture paused: no enabled sources");
                source_revision
                    .changed()
                    .await
                    .context("source revision channel closed")?;
                continue;
            }

            let ports = sources.iter().map(|source| source.port).collect::<Vec<_>>();
            let socket = AsyncFd::new(PacketSocket::open(&interface, &ports)?)
                .context("register packet socket with Tokio")?;
            tracing::info!(interface, ?ports, "capture filter installed");
            let mut frame = vec![0u8; 65_536];

            loop {
                tokio::select! {
                    changed = source_revision.changed() => {
                        changed.context("source revision channel closed")?;
                        tracing::info!("sources changed; rebuilding capture filter");
                        break;
                    }
                    ready = socket.readable() => {
                        let mut guard = ready.context("wait for packet socket")?;
                        match socket.get_ref().recv(&mut frame) {
                            Ok(Some((length, timestamp))) => {
                                if let Some(packet) = parse_ip_tcp(&frame[..length], timestamp) {
                                    if let Some(packet) = classify(packet, &sources) {
                                        let worker = worker_index("local", &packet.flow_key, workers.len());
                                        let ingress = IngressPacket { collector_id: "local".into(), packet };
                                        if workers[worker].try_send(ingress).is_err() {
                                            dropped_packets = dropped_packets.saturating_add(1);
                                            if dropped_packets.is_power_of_two() {
                                                tracing::warn!(dropped = dropped_packets, "flow worker queue is saturated");
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(None) => guard.clear_ready(),
                            Err(error) => return Err(error).context("receive packet"),
                        }
                    }
                }
            }
        }
    }

    struct PacketSocket {
        fd: OwnedFd,
    }

    impl PacketSocket {
        fn open(interface: &str, ports: &[u16]) -> Result<Self> {
            let interface = CString::new(interface).context("interface contains a NUL byte")?;
            let interface_index = unsafe { libc::if_nametoindex(interface.as_ptr()) };
            if interface_index == 0 {
                return Err(io::Error::last_os_error()).context("resolve capture interface");
            }

            let raw_fd = unsafe {
                libc::socket(
                    libc::AF_PACKET,
                    // SOCK_DGRAM strips interface-specific L2 headers, giving us
                    // the same IP layout on Ethernet, WireGuard/TUN and loopback.
                    libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                    i32::from(ETH_P_ALL.to_be()),
                )
            };
            if raw_fd < 0 {
                return Err(io::Error::last_os_error()).context("open AF_PACKET socket");
            }
            let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

            let mut address: libc::sockaddr_ll = unsafe { mem::zeroed() };
            address.sll_family = libc::AF_PACKET as u16;
            address.sll_protocol = ETH_P_ALL.to_be();
            address.sll_ifindex = interface_index as i32;
            let result = unsafe {
                libc::bind(
                    fd.as_raw_fd(),
                    (&address as *const libc::sockaddr_ll).cast(),
                    size_of::<libc::sockaddr_ll>() as libc::socklen_t,
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error()).context("bind AF_PACKET socket");
            }

            enable_kernel_timestamps(fd.as_raw_fd())?;
            attach_filter(fd.as_raw_fd(), ports)?;
            Ok(Self { fd })
        }

        fn recv(&self, buffer: &mut [u8]) -> io::Result<Option<(usize, i64)>> {
            let mut iovec = libc::iovec {
                iov_base: buffer.as_mut_ptr().cast(),
                iov_len: buffer.len(),
            };
            let mut control = [0u8; 128];
            let mut message: libc::msghdr = unsafe { mem::zeroed() };
            message.msg_iov = &mut iovec;
            message.msg_iovlen = 1;
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = control.len();

            let length = unsafe { libc::recvmsg(self.fd.as_raw_fd(), &mut message, 0) };
            if length < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(None);
                }
                return Err(error);
            }
            Ok(Some((length as usize, timestamp_from_message(&message))))
        }
    }

    impl AsRawFd for PacketSocket {
        fn as_raw_fd(&self) -> RawFd {
            self.fd.as_raw_fd()
        }
    }

    fn enable_kernel_timestamps(fd: RawFd) -> Result<()> {
        let enabled: libc::c_int = 1;
        let result = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_TIMESTAMPNS,
                (&enabled as *const libc::c_int).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error()).context("enable kernel packet timestamps");
        }
        Ok(())
    }

    fn timestamp_from_message(message: &libc::msghdr) -> i64 {
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::SOL_SOCKET
                    && (*header).cmsg_type == libc::SCM_TIMESTAMPNS
                    && (*header).cmsg_len
                        >= libc::CMSG_LEN(size_of::<libc::timespec>() as u32) as usize
                {
                    let timestamp =
                        std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<libc::timespec>());
                    return timestamp
                        .tv_sec
                        .saturating_mul(1_000_000)
                        .saturating_add(timestamp.tv_nsec / 1_000);
                }
                header = libc::CMSG_NXTHDR(message, header);
            }
        }
        super::unix_micros()
    }

    fn attach_filter(fd: RawFd, ports: &[u16]) -> Result<()> {
        if ports.is_empty() {
            bail!("cannot attach an empty capture filter");
        }
        let mut instructions = build_filter(ports);
        let program = libc::sock_fprog {
            len: instructions
                .len()
                .try_into()
                .context("capture filter is too large")?,
            filter: instructions.as_mut_ptr(),
        };
        let result = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                (&program as *const libc::sock_fprog).cast(),
                size_of::<libc::sock_fprog>() as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error()).context("attach kernel BPF filter");
        }
        Ok(())
    }

    fn build_filter(ports: &[u16]) -> Vec<libc::sock_filter> {
        let mut filter = vec![
            statement(BPF_LD | BPF_B | BPF_ABS, 0),
            statement(BPF_ALU | BPF_AND | BPF_K, 0xf0),
            jump(BPF_JMP | BPF_JEQ | BPF_K, 0x40, 1, 0),
            statement(BPF_JMP | BPF_JA, 0),
            statement(BPF_LD | BPF_B | BPF_ABS, 9),
            jump(BPF_JMP | BPF_JEQ | BPF_K, IPPROTO_TCP, 1, 0),
            statement(BPF_RET | BPF_K, 0),
            statement(BPF_LDX | BPF_B | BPF_MSH, 0),
            statement(BPF_LD | BPF_H | BPF_IND, 0),
        ];
        append_port_checks(&mut filter, ports);
        filter.push(statement(BPF_LD | BPF_H | BPF_IND, 2));
        append_port_checks(&mut filter, ports);
        filter.push(statement(BPF_RET | BPF_K, 0));

        let ipv6_start = filter.len();
        filter[3].k = (ipv6_start - 4) as u32;
        filter.extend([
            jump(BPF_JMP | BPF_JEQ | BPF_K, 0x60, 1, 0),
            statement(BPF_RET | BPF_K, 0),
            statement(BPF_LD | BPF_B | BPF_ABS, 6),
            jump(BPF_JMP | BPF_JEQ | BPF_K, IPPROTO_TCP, 1, 0),
            statement(BPF_RET | BPF_K, 0),
            statement(BPF_LD | BPF_H | BPF_ABS, 40),
        ]);
        append_port_checks(&mut filter, ports);
        filter.push(statement(BPF_LD | BPF_H | BPF_ABS, 42));
        append_port_checks(&mut filter, ports);
        filter.push(statement(BPF_RET | BPF_K, 0));
        filter
    }

    fn append_port_checks(filter: &mut Vec<libc::sock_filter>, ports: &[u16]) {
        for port in ports {
            filter.push(jump(BPF_JMP | BPF_JEQ | BPF_K, u32::from(*port), 0, 1));
            filter.push(statement(BPF_RET | BPF_K, u32::MAX));
        }
    }

    const fn statement(code: u16, k: u32) -> libc::sock_filter {
        libc::sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }

    const fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
        libc::sock_filter { code, jt, jf, k }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            build_filter, BPF_ABS, BPF_ALU, BPF_AND, BPF_B, BPF_H, BPF_IND, BPF_JA, BPF_JEQ,
            BPF_JMP, BPF_K, BPF_LD, BPF_LDX, BPF_MSH, BPF_RET,
        };

        #[test]
        fn cooked_filter_accepts_only_configured_tcp_ports() {
            let filter = build_filter(&[18_080]);
            let mut ipv4 = tcp_packet(4, 51_465, 18_080);
            assert_ne!(execute(&filter, &ipv4), 0);
            ipv4[22..24].copy_from_slice(&8080u16.to_be_bytes());
            assert_eq!(execute(&filter, &ipv4), 0);
            ipv4[20..22].copy_from_slice(&18_080u16.to_be_bytes());
            assert_ne!(execute(&filter, &ipv4), 0);

            let mut ipv6 = tcp_packet(6, 51_465, 18_080);
            assert_ne!(execute(&filter, &ipv6), 0);
            ipv6[42..44].copy_from_slice(&8080u16.to_be_bytes());
            assert_eq!(execute(&filter, &ipv6), 0);
        }

        fn tcp_packet(version: u8, source: u16, destination: u16) -> Vec<u8> {
            let mut packet = vec![0u8; if version == 4 { 40 } else { 60 }];
            packet[0] = (version << 4) | if version == 4 { 5 } else { 0 };
            let tcp = if version == 4 {
                packet[9] = 6;
                20
            } else {
                packet[6] = 6;
                40
            };
            packet[tcp..tcp + 2].copy_from_slice(&source.to_be_bytes());
            packet[tcp + 2..tcp + 4].copy_from_slice(&destination.to_be_bytes());
            packet
        }

        fn execute(filter: &[libc::sock_filter], packet: &[u8]) -> u32 {
            let (mut accumulator, mut index, mut pc) = (0u32, 0usize, 0usize);
            loop {
                let instruction = filter[pc];
                pc += 1;
                match instruction.code {
                    code if code == BPF_LD | BPF_B | BPF_ABS => {
                        accumulator = u32::from(packet[instruction.k as usize]);
                    }
                    code if code == BPF_LD | BPF_H | BPF_ABS => {
                        let offset = instruction.k as usize;
                        accumulator =
                            u32::from(u16::from_be_bytes([packet[offset], packet[offset + 1]]));
                    }
                    code if code == BPF_LD | BPF_H | BPF_IND => {
                        let offset = index + instruction.k as usize;
                        accumulator =
                            u32::from(u16::from_be_bytes([packet[offset], packet[offset + 1]]));
                    }
                    code if code == BPF_LDX | BPF_B | BPF_MSH => {
                        index = usize::from(packet[instruction.k as usize] & 0x0f) * 4;
                    }
                    code if code == BPF_ALU | BPF_AND | BPF_K => accumulator &= instruction.k,
                    code if code == BPF_JMP | BPF_JEQ | BPF_K => {
                        pc += usize::from(if accumulator == instruction.k {
                            instruction.jt
                        } else {
                            instruction.jf
                        });
                    }
                    code if code == BPF_JMP | BPF_JA => pc += instruction.k as usize,
                    code if code == BPF_RET | BPF_K => return instruction.k,
                    code => panic!("unsupported test instruction: {code:#x}"),
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn unix_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}
