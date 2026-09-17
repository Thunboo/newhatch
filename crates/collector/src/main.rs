use std::{env, sync::Arc};

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
    let receiver_addr = env::var("ANALYZER_CONNSTR").context("ANALYZER_CONNSTR is required")?;
    if receiver_addr.trim().is_empty() {
        bail!("ANALYZER_CONNSTR must be a host:port endpoint");
    }
    tokio::net::lookup_host(&receiver_addr)
        .await
        .with_context(|| format!("ANALYZER_CONNSTR cannot resolve {receiver_addr}"))?
        .next()
        .with_context(|| format!("ANALYZER_CONNSTR resolved no addresses for {receiver_addr}"))?;
    let capacity: usize = env::var("QUEUE_CAPACITY")
        .unwrap_or_else(|_| "8192".into())
        .parse()
        .context("invalid QUEUE_CAPACITY")?;
    if capacity == 0 {
        bail!("QUEUE_CAPACITY must be greater than zero");
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
