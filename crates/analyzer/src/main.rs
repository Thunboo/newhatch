use anyhow::Context;
use newhatch_analyzer::{
    api::{self, ApiState},
    auth::AuthConfig,
    capture,
    config::Config,
    flow::{self, FlowOptions},
    storage::{self, Catalog},
};
use regex::bytes::Regex;
use tokio::sync::watch;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
                    !metadata.target().starts_with("tower_sessions")
                        && !metadata.target().starts_with("tower_cookies")
                }))
                .with_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new("newhatch=info")),
                ),
        )
        .init();

    let auth = AuthConfig::from_env()?;
    let config = Config::from_env()?;
    let catalog = Catalog::open(config.sqlite_path())?;
    let storage = storage::start_writer(
        catalog.clone(),
        config.data_dir.clone(),
        config.segment_duration,
        config.segment_retention_count,
        config.storage_queue_capacity,
    )?;
    let flag_regex = Regex::new(&config.flag_regex).context("compile FLAG_REGEX")?;
    let flows = flow::start_workers(
        FlowOptions {
            workers: config.flow_workers,
            queue_capacity: config.packet_queue_capacity / config.flow_workers.max(1),
            idle_timeout: config.flow_idle_timeout,
            max_active_flows_per_worker: config.max_active_flows_per_worker,
            max_stream_bytes: config.max_stream_bytes,
            flag_regex: flag_regex.clone(),
        },
        storage,
    );

    let (source_revision, capture_revision) = watch::channel(0u64);
    let capture_interface = config.capture_interface.clone();
    let capture_catalog = catalog.clone();
    tokio::spawn(async move {
        if let Err(error) = capture::run(
            capture_interface,
            capture_catalog,
            capture_revision,
            flows.senders,
        )
        .await
        {
            tracing::error!(error = ?error, "capture task stopped");
        }
    });

    api::serve(
        config.listen_addr,
        ApiState {
            catalog,
            data_dir: config.data_dir,
            source_revision,
            flag_regex,
        },
        auth,
    )
    .await
}
