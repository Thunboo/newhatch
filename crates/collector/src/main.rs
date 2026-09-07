use std::{
    env,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use anyhow::{bail, Context};
use newhatch_collector::{
    capture::{self, CaptureCounters},
    transport,
};
use tokio::sync::{mpsc, watch};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("newhatch=info")),
        )
        .init();
    let collector_id = env::var("COLLECTOR_ID").unwrap_or_else(|_| "vulnbox-1".into());
    if collector_id.trim().is_empty() || collector_id.len() > 128 {
        bail!("COLLECTOR_ID must contain 1-128 bytes");
    }
    let interface = env::var("CAPTURE_INTERFACE").unwrap_or_else(|_| "eth0".into());
    let receiver_ip: IpAddr = env::var("RECEIVER_ADDR")
        .context("RECEIVER_ADDR is required")?
        .parse()
        .context("RECEIVER_ADDR must be an IP address")?;
    let receiver_port: u16 = env::var("RECEIVER_PORT")
        .unwrap_or_else(|_| "39090".into())
        .parse()
        .context("invalid RECEIVER_PORT")?;
    let receiver_addr = SocketAddr::new(receiver_ip, receiver_port);
    let capacity: usize = env::var("COLLECTOR_QUEUE_CAPACITY")
        .unwrap_or_else(|_| "8192".into())
        .parse()
        .context("invalid COLLECTOR_QUEUE_CAPACITY")?;
    if capacity == 0 {
        bail!("COLLECTOR_QUEUE_CAPACITY must be greater than zero");
    }

    let (packet_tx, packet_rx) = mpsc::channel(capacity);
    let (source_tx, source_rx) = watch::channel(Arc::new(Vec::new()));
    let counters = Arc::new(CaptureCounters::default());
    let capture = tokio::spawn(capture::run(
        interface,
        source_rx,
        packet_tx,
        counters.clone(),
    ));
    let transport = tokio::spawn(transport::run(
        collector_id,
        receiver_addr,
        packet_rx,
        source_tx,
        counters,
        capacity,
    ));
    tokio::select! {
        result = capture => result.context("capture task panicked")??,
        result = transport => result.context("transport task panicked")??,
        signal = tokio::signal::ctrl_c() => signal.context("install shutdown signal")?,
    }
    Ok(())
}
