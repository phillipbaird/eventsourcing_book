use std::hash::{DefaultHasher, Hash, Hasher};

use disintegrate::EventStore;
use rdkafka::{
    ClientConfig,
    error::KafkaError,
    producer::{FutureProducer, FutureRecord, Producer},
    util::Timeout,
};
use thiserror::Error;

use crate::{domain::DomainEvent, infra::KafkaSettings};

pub async fn publish_with_events<T>(
    settings: &KafkaSettings,
    event_store: &crate::domain::EventStore,
    topic: String,
    message: T,
    success_event: DomainEvent,
) -> Result<(), PublishError>
where
    T: serde::Serialize + Hash,
{
    let producer = create_transactional_producer(settings, topic.clone())?;
    let payload = serde_json::to_vec(&message)?;
    let result: Result<(), PublishError> = {
        producer
            .send(
                FutureRecord::to(&topic)
                    .payload(&payload)
                    .key(&calculate_hash(&message).to_string()),
                Timeout::Never,
            )
            .await
            .map_err(|(e, _)| e)?;
        event_store
            .append_without_validation(vec![success_event])
            .await?;
        Ok(())
    };
    match result {
        Ok(_) => {
            producer.commit_transaction(Timeout::Never)?;
            Ok(())
        }
        Err(e) => {
            producer.flush(Timeout::Never)?;
            producer.abort_transaction(Timeout::Never)?;
            Err(e)
        }
    }
}

/// Constructs a Kafka producer for sending messages/events.
fn create_transactional_producer(
    settings: &KafkaSettings,
    transactional_id: String,
) -> Result<FutureProducer, KafkaError> {
    ClientConfig::new()
        .set("bootstrap.servers", settings.bootstrap_servers.clone())
        .set(
            "message.timeout.ms",
            settings.session_timeout_ms.to_string(),
        )
        .set("enable.idempotence", "true")
        .set("transactional.id", transactional_id)
        .create::<FutureProducer>()
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

#[derive(Debug, Error)]
pub enum PublishError {
    #[error(transparent)]
    Kafka(#[from] KafkaError),
    #[error(transparent)]
    EventStore(#[from] disintegrate_postgres::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}
