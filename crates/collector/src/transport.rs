use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use newhatch_protocol::{
    envelope::Body, read_frame, write_frame, ClassifiedPacket, CollectorHello, Envelope, Heartbeat,
    PacketMessage, Source, Stats,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, watch},
};

use crate::capture::CaptureCounters;

struct RuntimeStats {
    sent: AtomicU64,
    reconnects: AtomicU64,
    queue_capacity: usize,
}

pub async fn run(
    collector_id: String,
    receiver_addr: std::net::SocketAddr,
    mut packets: mpsc::Receiver<ClassifiedPacket>,
    sources: watch::Sender<Arc<Vec<Source>>>,
    counters: Arc<CaptureCounters>,
    queue_capacity: usize,
) -> Result<()> {
    let stats = RuntimeStats {
        sent: AtomicU64::new(0),
        reconnects: AtomicU64::new(0),
        queue_capacity,
    };
    let mut backoff = Duration::from_millis(250);
    loop {
        match connect_once(
            &collector_id,
            receiver_addr,
            &mut packets,
            &sources,
            &counters,
            &stats,
        )
        .await
        {
            Ok(()) => tracing::warn!("receiver closed the connection"),
            Err(error) => {
                tracing::warn!(error = ?error, %receiver_addr, "collector transport disconnected")
            }
        }
        stats.reconnects.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(10));
    }
}

async fn connect_once(
    collector_id: &str,
    receiver_addr: std::net::SocketAddr,
    packets: &mut mpsc::Receiver<ClassifiedPacket>,
    sources: &watch::Sender<Arc<Vec<Source>>>,
    counters: &CaptureCounters,
    stats: &RuntimeStats,
) -> Result<()> {
    let mut stream = TcpStream::connect(receiver_addr)
        .await
        .with_context(|| format!("connect to receiver {receiver_addr}"))?;
    stream.set_nodelay(true).context("set TCP_NODELAY")?;
    write_frame(
        &mut stream,
        &Envelope::new(Body::CollectorHello(CollectorHello {
            collector_id: collector_id.to_owned(),
        })),
    )
    .await?;
    let ack = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stream))
        .await
        .context("receiver HelloAck timeout")??;
    if !matches!(ack.body, Some(Body::HelloAck(_))) {
        anyhow::bail!("receiver did not acknowledge collector");
    }
    receive_sources(&mut stream, sources).await?;
    tracing::info!(%receiver_addr, collector_id, "collector connected");

    let (mut reader, mut writer) = stream.into_split();
    let mut report = tokio::time::interval(Duration::from_secs(2));
    report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            packet = packets.recv() => {
                let packet = packet.context("capture queue closed")?;
                write_frame(&mut writer, &Envelope::new(Body::Packet(PacketMessage::from(&packet)))).await?;
                stats.sent.fetch_add(1, Ordering::Relaxed);
            }
            message = read_frame(&mut reader) => match message?.body {
                Some(Body::SetSources(update)) => {
                    let values = update.sources.into_iter().map(Source::try_from).collect::<Result<Vec<_>, _>>()?;
                    sources.send_replace(Arc::new(values));
                    tracing::info!(revision = update.revision, "collector sources updated");
                }
                Some(Body::Heartbeat(_)) => {}
                _ => anyhow::bail!("unexpected receiver message"),
            },
            _ = report.tick() => {
                let stats = Stats {
                    captured_packets: counters.captured.load(Ordering::Relaxed),
                    sent_packets: stats.sent.load(Ordering::Relaxed),
                    dropped_packets: counters.dropped.load(Ordering::Relaxed),
                    queue_depth: stats.queue_capacity.saturating_sub(packets.capacity()) as u64,
                    reconnect_count: stats.reconnects.load(Ordering::Relaxed),
                };
                write_frame(&mut writer, &Envelope::new(Body::Stats(stats))).await?;
                write_frame(&mut writer, &Envelope::new(Body::Heartbeat(Heartbeat { timestamp_micros: unix_micros() }))).await?;
            }
        }
    }
}

async fn receive_sources(
    stream: &mut TcpStream,
    sources: &watch::Sender<Arc<Vec<Source>>>,
) -> Result<()> {
    let message = tokio::time::timeout(Duration::from_secs(5), read_frame(stream))
        .await
        .context("SetSources timeout")??;
    let update = match message.body {
        Some(Body::SetSources(update)) => update,
        _ => anyhow::bail!("receiver did not send Sources"),
    };
    let values = update
        .sources
        .into_iter()
        .map(Source::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    sources.send_replace(Arc::new(values));
    Ok(())
}

fn unix_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_micros().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::capture::CaptureCounters;
    use newhatch_protocol::ClassifiedPacket;
    use std::sync::{atomic::Ordering, Arc};
    use tokio::sync::mpsc;

    #[test]
    fn bounded_queue_reports_overflow() {
        let (sender, _receiver) = mpsc::channel::<ClassifiedPacket>(1);
        let counters = Arc::new(CaptureCounters::default());
        let _permit = sender.try_reserve().unwrap();
        if sender.try_reserve().is_err() {
            counters.dropped.fetch_add(1, Ordering::Relaxed);
        }
        assert_eq!(counters.dropped.load(Ordering::Relaxed), 1);
    }
}
