use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result};
use newhatch_protocol::{
    envelope::Body, read_frame, write_frame, ClassifiedPacket, Envelope, HelloAck, SetSources,
    Stats, WireSource,
};
use serde::Serialize;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
};

use crate::{
    flow::{worker_index, IngressPacket},
    storage::Catalog,
};

#[derive(Clone, Default)]
pub struct CollectorRegistry(Arc<RwLock<HashMap<String, CollectorStatus>>>);

#[derive(Clone, Debug, Serialize)]
pub struct CollectorStatus {
    pub collector_id: String,
    pub connected: bool,
    pub peer: String,
    pub connected_at: i64,
    pub last_activity: i64,
    pub captured_packets: u64,
    pub sent_packets: u64,
    pub dropped_packets: u64,
    pub queue_depth: u64,
    pub reconnect_count: u64,
    pub receiver_dropped_packets: u64,
    pub last_error: Option<String>,
}

impl CollectorRegistry {
    pub fn list(&self) -> Vec<CollectorStatus> {
        let mut values = self
            .0
            .read()
            .expect("collector registry poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|a, b| a.collector_id.cmp(&b.collector_id));
        values
    }

    fn connected(&self, id: &str, peer: SocketAddr) {
        let now = unix_micros();
        self.0.write().expect("collector registry poisoned").insert(
            id.to_owned(),
            CollectorStatus {
                collector_id: id.to_owned(),
                connected: true,
                peer: peer.to_string(),
                connected_at: now,
                last_activity: now,
                captured_packets: 0,
                sent_packets: 0,
                dropped_packets: 0,
                queue_depth: 0,
                reconnect_count: 0,
                receiver_dropped_packets: 0,
                last_error: None,
            },
        );
    }

    fn update_stats(&self, id: &str, stats: Stats) {
        if let Some(status) = self
            .0
            .write()
            .expect("collector registry poisoned")
            .get_mut(id)
        {
            status.last_activity = unix_micros();
            status.captured_packets = stats.captured_packets;
            status.sent_packets = stats.sent_packets;
            status.dropped_packets = stats.dropped_packets;
            status.queue_depth = stats.queue_depth;
            status.reconnect_count = stats.reconnect_count;
        }
    }

    fn activity(&self, id: &str) {
        if let Some(status) = self
            .0
            .write()
            .expect("collector registry poisoned")
            .get_mut(id)
        {
            status.last_activity = unix_micros();
        }
    }

    fn receiver_drop(&self, id: &str) {
        if let Some(status) = self
            .0
            .write()
            .expect("collector registry poisoned")
            .get_mut(id)
        {
            status.receiver_dropped_packets = status.receiver_dropped_packets.saturating_add(1);
        }
    }

    fn disconnected(&self, id: &str, error: Option<String>) {
        if let Some(status) = self
            .0
            .write()
            .expect("collector registry poisoned")
            .get_mut(id)
        {
            status.connected = false;
            status.last_activity = unix_micros();
            status.last_error = error;
        }
    }
}

pub async fn run_receiver(
    address: SocketAddr,
    allowed_ips: Vec<IpAddr>,
    catalog: Catalog,
    source_revision: watch::Receiver<u64>,
    workers: Vec<mpsc::Sender<IngressPacket>>,
    registry: CollectorRegistry,
) -> Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("bind collector receiver to {address}"))?;
    let allowed_ips = Arc::new(allowed_ips.into_iter().collect::<HashSet<_>>());
    tracing::info!(%address, ?allowed_ips, "collector receiver listening");
    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .context("accept collector connection")?;
        if !peer_is_allowed(&allowed_ips, peer.ip()) {
            tracing::warn!(%peer, "rejected unmanifested collector peer");
            continue;
        }
        let task_catalog = catalog.clone();
        let task_revision = source_revision.clone();
        let task_workers = workers.clone();
        let task_registry = registry.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(
                stream,
                peer,
                task_catalog,
                task_revision,
                task_workers,
                task_registry,
            )
            .await
            {
                tracing::warn!(%peer, error = ?error, "collector connection closed");
            }
        });
    }
}

fn peer_is_allowed(allowed: &HashSet<IpAddr>, peer: IpAddr) -> bool {
    allowed.contains(&peer)
}

async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    catalog: Catalog,
    mut source_revision: watch::Receiver<u64>,
    workers: Vec<mpsc::Sender<IngressPacket>>,
    registry: CollectorRegistry,
) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    let hello = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut reader))
        .await
        .context("collector Hello timeout")??;
    let collector_id = match hello.body {
        Some(Body::CollectorHello(hello))
            if !hello.collector_id.trim().is_empty() && hello.collector_id.len() <= 128 =>
        {
            hello.collector_id
        }
        _ => anyhow::bail!("first collector message must be a valid CollectorHello"),
    };
    registry.connected(&collector_id, peer);
    let result = async {
        write_frame(&mut writer, &Envelope::new(Body::HelloAck(HelloAck { server_time_micros: unix_micros() }))).await?;
        let revision = *source_revision.borrow();
        let mut active_source_ids = send_sources(&catalog, revision, &mut writer).await?;
        loop {
            tokio::select! {
                message = read_frame(&mut reader) => match message?.body {
                    Some(Body::Packet(packet)) => {
                        let packet = ClassifiedPacket::try_from(packet)?;
                        if !active_source_ids.contains(&packet.source_id) { continue; }
                        registry.activity(&collector_id);
                        let worker = worker_index(&collector_id, &packet.flow_key, workers.len());
                        if workers[worker].try_send(IngressPacket { collector_id: collector_id.clone(), packet }).is_err() { registry.receiver_drop(&collector_id); }
                    }
                    Some(Body::Stats(stats)) => registry.update_stats(&collector_id, stats),
                    Some(Body::Heartbeat(_)) => registry.activity(&collector_id),
                    Some(Body::ErrorStatus(error)) => tracing::warn!(collector_id, message = error.message, "collector reported an error"),
                    _ => anyhow::bail!("unexpected collector message"),
                },
                changed = source_revision.changed() => {
                    changed.context("source revision channel closed")?;
                    let revision = *source_revision.borrow();
                    active_source_ids = send_sources(&catalog, revision, &mut writer).await?;
                }
            }
        }
    }.await;
    registry.disconnected(
        &collector_id,
        result.as_ref().err().map(ToString::to_string),
    );
    result
}

async fn send_sources<W: tokio::io::AsyncWrite + Unpin>(
    catalog: &Catalog,
    revision: u64,
    writer: &mut W,
) -> Result<HashSet<i64>> {
    let catalog = catalog.clone();
    let sources = tokio::task::spawn_blocking(move || catalog.list_sources(true))
        .await
        .context("source loader panicked")??;
    let ids = sources.iter().map(|source| source.id).collect();
    let sources = sources.iter().map(WireSource::from).collect();
    write_frame(
        writer,
        &Envelope::new(Body::SetSources(SetSources { revision, sources })),
    )
    .await?;
    Ok(ids)
}

fn unix_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_micros().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{handle_connection, peer_is_allowed, CollectorRegistry};
    use crate::{domain::SourceInput, flow::IngressPacket, storage::Catalog};
    use newhatch_protocol::{
        envelope::Body, read_frame, write_frame, ClassifiedPacket, CollectorHello, Direction,
        Endpoint, Envelope, FlowKey, PacketMessage, Stats, TcpPacket,
    };
    use std::{collections::HashSet, time::Duration};
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::{mpsc, watch},
    };

    #[test]
    fn registry_tracks_disconnect_and_stats() {
        let registry = CollectorRegistry::default();
        registry.connected("one", "127.0.0.1:1234".parse().unwrap());
        registry.update_stats(
            "one",
            Stats {
                captured_packets: 4,
                sent_packets: 3,
                dropped_packets: 1,
                queue_depth: 0,
                reconnect_count: 2,
            },
        );
        registry.disconnected("one", None);
        let status = registry.list().pop().unwrap();
        assert!(!status.connected);
        assert_eq!(status.captured_packets, 4);
        assert_eq!(status.reconnect_count, 2);
    }

    #[test]
    fn admits_only_manifested_peer_ip() {
        let allowed = HashSet::from(["100.97.69.83".parse().unwrap()]);
        assert!(peer_is_allowed(&allowed, "100.97.69.83".parse().unwrap()));
        assert!(!peer_is_allowed(&allowed, "100.97.69.84".parse().unwrap()));
    }

    #[tokio::test]
    async fn handshake_pushes_sources_updates_and_forwards_packets() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path().join("index.sqlite")).unwrap();
        let source = catalog
            .create_source(&SourceInput {
                name: "web".into(),
                port: 8080,
                enabled: true,
            })
            .unwrap();
        let (revision_tx, revision_rx) = watch::channel(0u64);
        let (worker_tx, mut worker_rx) = mpsc::channel::<IngressPacket>(8);
        let registry = CollectorRegistry::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_catalog = catalog.clone();
        let server_registry = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(
                stream,
                peer,
                server_catalog,
                revision_rx,
                vec![worker_tx],
                server_registry,
            )
            .await
        });

        let mut client = TcpStream::connect(address).await.unwrap();
        write_frame(
            &mut client,
            &Envelope::new(Body::CollectorHello(CollectorHello {
                collector_id: "test-box".into(),
            })),
        )
        .await
        .unwrap();
        assert!(matches!(
            read_frame(&mut client).await.unwrap().body,
            Some(Body::HelloAck(_))
        ));
        let initial = read_frame(&mut client).await.unwrap();
        assert!(
            matches!(initial.body, Some(Body::SetSources(ref update)) if update.sources.len() == 1)
        );

        catalog
            .create_source(&SourceInput {
                name: "ssh".into(),
                port: 22,
                enabled: true,
            })
            .unwrap();
        revision_tx.send_modify(|value| *value += 1);
        let update = read_frame(&mut client).await.unwrap();
        assert!(
            matches!(update.body, Some(Body::SetSources(ref sources)) if sources.sources.len() == 2)
        );

        let flow_key = FlowKey {
            client: Endpoint {
                ip: "10.0.0.2".parse().unwrap(),
                port: 50000,
            },
            server: Endpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
            },
        };
        let packet = ClassifiedPacket {
            source_id: source.id,
            direction: Direction::C2s,
            flow_key: flow_key.clone(),
            packet: TcpPacket {
                timestamp_micros: 1,
                src: flow_key.client,
                dst: flow_key.server,
                sequence: 10,
                syn: false,
                fin: false,
                rst: false,
                payload: b"hello".to_vec(),
            },
        };
        write_frame(
            &mut client,
            &Envelope::new(Body::Packet(PacketMessage::from(&packet))),
        )
        .await
        .unwrap();
        let ingress = tokio::time::timeout(Duration::from_secs(1), worker_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ingress.collector_id, "test-box");
        assert_eq!(ingress.packet.packet.payload, b"hello");
        drop(client);
        assert!(server.await.unwrap().is_err());
        assert!(!registry.list()[0].connected);
    }
}
