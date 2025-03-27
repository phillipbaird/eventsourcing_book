pub mod cart;
mod events;
mod helpers;

pub use events::{CartStream, DomainEvent, EmptyStream, InventoryStream, PricingStream};
pub use helpers::{
    fake::*,
    device_fingerprint_calculator::default_fingerprint,
    PublishError,
    live_read_models::{EventReadingError, read_from_events},
};

use disintegrate::{WithSnapshot, serde::json::Json};
use disintegrate_postgres::{Error, PgEventStore, PgSnapshotter, decision_maker};
use sqlx::PgPool;

pub type DecisionMaker = disintegrate_postgres::PgDecisionMaker<
    events::DomainEvent,
    Json<events::DomainEvent>,
    disintegrate_postgres::WithPgSnapshot,
>;

pub type EventStore = PgEventStore<DomainEvent, Json<DomainEvent>>;

pub async fn create_eventstore(pool: &PgPool) -> Result<EventStore, Error> {
    PgEventStore::new(pool.clone(), Json::<DomainEvent>::default()).await
}

pub async fn create_eventstore_and_decider(
    pool: &PgPool,
) -> Result<(EventStore, DecisionMaker), Error> {
    let event_store = create_eventstore(pool).await?;
    let snapshotter = PgSnapshotter::new(pool.clone(), 100).await?;
    let decider = decision_maker(event_store.clone(), WithSnapshot::new(snapshotter));
    Ok((event_store, decider))
}
