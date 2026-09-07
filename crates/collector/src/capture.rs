use std::sync::{atomic::AtomicU64, Arc};

#[cfg(not(target_os = "linux"))]
use anyhow::Context;
use anyhow::Result;
use newhatch_protocol::{ClassifiedPacket, Source};
use tokio::sync::{mpsc, watch};

use crate::packet::{classify, parse_ip_tcp};

#[derive(Default)]
pub struct CaptureCounters {
    pub captured: AtomicU64,
    pub dropped: AtomicU64,
}

pub async fn run(
    interface: String,
    mut sources: watch::Receiver<Arc<Vec<Source>>>,
    sender: mpsc::Sender<ClassifiedPacket>,
    counters: Arc<CaptureCounters>,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::run(interface, &mut sources, sender, counters).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interface, sender, counters);
        tracing::warn!("AF_PACKET capture is available only on Linux");
        loop {
            sources.changed().await.context("source channel closed")?;
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{classify, parse_ip_tcp, CaptureCounters};
    use anyhow::{bail, Context, Result};
    use newhatch_protocol::{ClassifiedPacket, Source};
    use std::{
        ffi::CString,
        io,
        mem::{self, size_of},
        os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        sync::{atomic::Ordering, Arc},
    };
    use tokio::{
        io::unix::AsyncFd,
        sync::{mpsc, watch},
    };

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
        sources_rx: &mut watch::Receiver<Arc<Vec<Source>>>,
        sender: mpsc::Sender<ClassifiedPacket>,
        counters: Arc<CaptureCounters>,
    ) -> Result<()> {
        loop {
            let sources = sources_rx.borrow().clone();
            if sources.is_empty() {
                tracing::info!("capture paused: waiting for enabled sources");
                sources_rx
                    .changed()
                    .await
                    .context("source channel closed")?;
                continue;
            }
            let ports = sources.iter().map(|source| source.port).collect::<Vec<_>>();
            let socket = AsyncFd::new(PacketSocket::open(&interface, &ports)?)
                .context("register packet socket")?;
            tracing::info!(interface, ?ports, "collector capture filter installed");
            let mut frame = vec![0u8; 65_536];
            loop {
                tokio::select! {
                    changed = sources_rx.changed() => { changed.context("source channel closed")?; break; }
                    ready = socket.readable() => {
                        let mut guard = ready.context("wait for packet socket")?;
                        match socket.get_ref().recv(&mut frame) {
                            Ok(Some((length, timestamp))) => {
                                counters.captured.fetch_add(1, Ordering::Relaxed);
                                if let Some(packet) = parse_ip_tcp(&frame[..length], timestamp).and_then(|packet| classify(packet, &sources)) {
                                    if sender.try_send(packet).is_err() { counters.dropped.fetch_add(1, Ordering::Relaxed); }
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
            let interface = CString::new(interface).context("interface contains NUL")?;
            let index = unsafe { libc::if_nametoindex(interface.as_ptr()) };
            if index == 0 {
                return Err(io::Error::last_os_error()).context("resolve capture interface");
            }
            let raw = unsafe {
                libc::socket(
                    libc::AF_PACKET,
                    libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                    i32::from(ETH_P_ALL.to_be()),
                )
            };
            if raw < 0 {
                return Err(io::Error::last_os_error()).context("open AF_PACKET socket");
            }
            let fd = unsafe { OwnedFd::from_raw_fd(raw) };
            let mut address: libc::sockaddr_ll = unsafe { mem::zeroed() };
            address.sll_family = libc::AF_PACKET as u16;
            address.sll_protocol = ETH_P_ALL.to_be();
            address.sll_ifindex = index as i32;
            if unsafe {
                libc::bind(
                    fd.as_raw_fd(),
                    (&address as *const libc::sockaddr_ll).cast(),
                    size_of::<libc::sockaddr_ll>() as libc::socklen_t,
                )
            } != 0
            {
                return Err(io::Error::last_os_error()).context("bind AF_PACKET socket");
            }
            enable_timestamps(fd.as_raw_fd())?;
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
            Ok(Some((length as usize, timestamp(&message))))
        }
    }
    impl AsRawFd for PacketSocket {
        fn as_raw_fd(&self) -> RawFd {
            self.fd.as_raw_fd()
        }
    }

    fn enable_timestamps(fd: RawFd) -> Result<()> {
        let enabled: libc::c_int = 1;
        if unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_TIMESTAMPNS,
                (&enabled as *const libc::c_int).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        } != 0
        {
            return Err(io::Error::last_os_error()).context("enable packet timestamps");
        }
        Ok(())
    }
    fn timestamp(message: &libc::msghdr) -> i64 {
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::SOL_SOCKET
                    && (*header).cmsg_type == libc::SCM_TIMESTAMPNS
                {
                    let value =
                        std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<libc::timespec>());
                    return value
                        .tv_sec
                        .saturating_mul(1_000_000)
                        .saturating_add(value.tv_nsec / 1_000);
                }
                header = libc::CMSG_NXTHDR(message, header);
            }
        }
        unix_micros()
    }
    fn attach_filter(fd: RawFd, ports: &[u16]) -> Result<()> {
        if ports.is_empty() {
            bail!("empty capture filter");
        }
        let mut instructions = build_filter(ports);
        let program = libc::sock_fprog {
            len: instructions
                .len()
                .try_into()
                .context("capture filter too large")?,
            filter: instructions.as_mut_ptr(),
        };
        if unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                (&program as *const libc::sock_fprog).cast(),
                size_of::<libc::sock_fprog>() as libc::socklen_t,
            )
        } != 0
        {
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
        checks(&mut filter, ports);
        filter.push(statement(BPF_LD | BPF_H | BPF_IND, 2));
        checks(&mut filter, ports);
        filter.push(statement(BPF_RET | BPF_K, 0));
        let ipv6 = filter.len();
        filter[3].k = (ipv6 - 4) as u32;
        filter.extend([
            jump(BPF_JMP | BPF_JEQ | BPF_K, 0x60, 1, 0),
            statement(BPF_RET | BPF_K, 0),
            statement(BPF_LD | BPF_B | BPF_ABS, 6),
            jump(BPF_JMP | BPF_JEQ | BPF_K, IPPROTO_TCP, 1, 0),
            statement(BPF_RET | BPF_K, 0),
            statement(BPF_LD | BPF_H | BPF_ABS, 40),
        ]);
        checks(&mut filter, ports);
        filter.push(statement(BPF_LD | BPF_H | BPF_ABS, 42));
        checks(&mut filter, ports);
        filter.push(statement(BPF_RET | BPF_K, 0));
        filter
    }
    fn checks(filter: &mut Vec<libc::sock_filter>, ports: &[u16]) {
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
    fn unix_micros() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros().min(i64::MAX as u128) as i64)
            .unwrap_or_default()
    }
}
