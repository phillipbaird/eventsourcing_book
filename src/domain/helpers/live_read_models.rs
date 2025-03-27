use disintegrate::{Event, EventId, EventStore, StreamQuery};
use futures::stream::StreamExt;

use crate::infra::ClientError;

/// A utility function for creating read models directly from events rather than being precomputed and stored in the database.
/// It projects (folds) over a stream of events, reading from the eventstore, producing a read model as output.
pub async fn read_from_events<RM, ID, E, ES, QE, AE>(
    event_store: &ES,
    query: &StreamQuery<ID, QE>,
    initial_read_model: RM,
    apply_event_fn: AE,
) -> Result<RM, EventReadingError<ID, ES::Error>>
where
    ID: EventId,
    E: Event + Clone + Send + Sync + 'static,
    ES: EventStore<ID, E>,
    QE: TryFrom<E> + Event + 'static + Clone + Send + Sync,
    <QE as TryFrom<E>>::Error: std::error::Error + 'static + Send + Sync,
    AE: Fn(RM, QE) -> RM,
{
    let mut events_stream = event_store.stream(query);
    let mut last_processed_event_id: ID = Default::default();
    let mut read_model = initial_read_model;

    while let Some(event) = events_stream.next().await {
        let event = event.map_err(|err| EventReadingError::ReadFromEventsError {
            last_processed_event_id,
            source: err,
        })?;
        let event_id = event.id();
        read_model = apply_event_fn(read_model, event.into_inner());
        last_processed_event_id = event_id;
    }

    Ok(read_model)
}

#[derive(Debug, thiserror::Error)]
pub enum EventReadingError<ID: EventId, ERR> {
    #[error("Reading event failed. Last successfully read event: {last_processed_event_id}")]
    ReadFromEventsError {
        last_processed_event_id: ID,
        source: ERR,
    },
    #[error("Could not read last processed event id for event listener: {listener_id}")]
    CannotReadLastProcessedId {
        listener_id: String,
        source: sqlx::Error,
    },
}

impl<
    ID: EventId + std::fmt::Debug + std::fmt::Display,
    Err: std::error::Error + Send + Sync + 'static,
> From<EventReadingError<ID, Err>> for ClientError
{
    fn from(value: EventReadingError<ID, Err>) -> Self {
        ClientError::Internal(anyhow::Error::new(value))
    }
}
