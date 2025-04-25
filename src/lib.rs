pub mod domain;
pub mod infra;
pub mod subsystems;

use std::time::Duration;

use anyhow::Context;
use axum::extract::FromRef;
use domain::{create_eventstore_and_decider, DecisionMaker, EventStore};
use infra::{DatabaseSettings, Settings};
use sqlx::{postgres::PgPoolOptions, PgPool};
use subsystems::{
    work_queue::WorkQueue, EventListeners, KafkaListeners, WebServer, WorkQueueSubsystem,
};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, Toplevel};
use tracing_appender::non_blocking::WorkerGuard;

#[derive(Clone, FromRef)]
pub struct AppState {
    pub settings: Settings,
    pub pool: PgPool,
    pub decider: DecisionMaker,
    pub event_store: EventStore,
    pub work_queue: WorkQueue,
}

pub fn build_subsystems(state: AppState) -> Toplevel {
    let event_listeners = EventListeners::new(state.clone());
    let kafka_listeners = KafkaListeners::new(state.clone());
    let work_queue_subsystem = WorkQueueSubsystem::new(state.clone());
    let webserver = WebServer::new(state);

    // Setup and execute subsystem tree
    Toplevel::new(async |s| {
        s.start(SubsystemBuilder::new(
            "EventListeners",
            event_listeners.into_subsystem(),
        ));
        s.start(SubsystemBuilder::new(
            "KafkaListeners",
            kafka_listeners.into_subsystem(),
        ));
        s.start(SubsystemBuilder::new(
            "WorkQueue",
            work_queue_subsystem.into_subsystem(),
        ));
        s.start(SubsystemBuilder::new(
            "Webserver",
            webserver.into_subsystem(),
        ));
    })
}

pub async fn test_server(toplevel: Toplevel, pool: PgPool) -> anyhow::Result<()> {
    let result = toplevel
        .handle_shutdown_requests(Duration::from_millis(2000))
        .await
        .map_err(Into::into);
    pool.close().await;
    result
}

pub async fn start_server(state: AppState) -> anyhow::Result<()> {
    build_subsystems(state)
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(2000))
        .await
        .map_err(Into::into)
}

pub fn configure_tracing(settings: &Settings) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(
        settings.application.logs_directory.clone(),
        "cart_server.log",
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(non_blocking)
        .init();
    _guard
}

pub async fn construct_app_state(settings: Settings) -> Result<AppState, anyhow::Error> {
    let pool = construct_db_pool(&settings.database).await?;
    let (event_store, decider) = create_eventstore_and_decider(&pool).await?;
    let work_queue = WorkQueue::new(pool.clone());

    Ok(AppState {
        settings,
        pool,
        event_store,
        decider,
        work_queue,
    })
}

pub async fn construct_db_pool(settings: &DatabaseSettings) -> Result<PgPool, anyhow::Error> {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_with(settings.with_db_name())
        .await
        .context("Failed to connect to Postgres database.\n1. Check database is running.\n2. Check Postgres database settings in configuration file(s).")
}
