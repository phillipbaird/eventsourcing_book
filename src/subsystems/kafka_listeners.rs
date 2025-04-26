use anyhow::{Context, anyhow};
use async_trait::async_trait;
use backon::{ExponentialBuilder, Retryable};
use futures::FutureExt;
use rdkafka::{
    ClientConfig, Message,
    consumer::{CommitMode, Consumer, StreamConsumer},
};
use sqlx::PgPool;
use tokio::select;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};
use tracing::{error, info, warn};

use crate::{
    AppState,
    domain::cart::{InventoryChangedTranslator, PriceChangeTranslator},
    infra::KafkaSettings,
};

/// The trait that must be implemented for any type that can handle Kafka events/messages.
/// An KafkaMessageHandler is normally expected to translate an external message to
/// a command responsible for injesting the message as events into the domain.
/// One of the decisions the command may need to make is determing if the received message has
/// already been processed.
#[async_trait]
pub trait KafkaMessageHandler {
    type Message: Send;
    const TOPIC: &str;
    const GROUP: &str;
    /// The handle_message implementations are responsible for all error handling.
    /// Whether that is simply logging the issue or sending the message to a deadletter queue.
    async fn handle_message(&self, offset: i64, message: Self::Message);
}

//--------------------- All Kafka Listeners ----------------------

pub struct KafkaListeners {
    state: AppState,
}

impl KafkaListeners {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for KafkaListeners {
    async fn run(self, subsys: SubsystemHandle) -> Result<(), anyhow::Error> {
        let listener = KafkaListener {
            name: "InventoryChangedMessageHander".to_owned(),
            pool: self.state.pool.clone(),
            settings: self.state.settings.kafka.clone(),
            handler: InventoryChangedTranslator::new(self.state.decider.clone()),
        };
        subsys.start(SubsystemBuilder::new(
            listener.name.clone(),
            listener.into_subsystem(),
        ));

        let listener = KafkaListener {
            name: "PriceChangedMessageHander".to_owned(),
            pool: self.state.pool,
            settings: self.state.settings.kafka,
            handler: PriceChangeTranslator::new(self.state.decider),
        };
        subsys.start(SubsystemBuilder::new(
            listener.name.clone(),
            listener.into_subsystem(),
        ));

        subsys.on_shutdown_requested().await;

        Ok(())
    }
}

//----------------- Kafka Listener Implementation ------------------

struct KafkaListener<H>
where
    H: KafkaMessageHandler + Clone + Send + Sync + 'static,
    for<'de> <H as KafkaMessageHandler>::Message: serde::Deserialize<'de>,
{
    pub name: String,
    pub pool: PgPool,
    pub settings: KafkaSettings,
    pub handler: H,
}

impl<H> KafkaListener<H>
where
    H: KafkaMessageHandler + Clone + Send + Sync + 'static,
    for<'de> <H as KafkaMessageHandler>::Message: serde::Deserialize<'de>,
{
    /// Log and restart Kafka Listener if an error occurs.
    /// Uses exponential backoff if the problem persists.
    async fn listen(&self) -> Result<(), anyhow::Error> {
        (|| async { self.try_listen().await })
            .retry(ExponentialBuilder::default())
            .when(|_err| true) // TODO Review errors that could occur and determine which should allow
            // for restart, e.g. Kafka being offline.
            .sleep(tokio::time::sleep)
            .notify(|err, dur| error!("Restarting {} due to: {err:?} after {dur:?}", self.name))
            .await
    }

    async fn try_listen(&self) -> Result<(), anyhow::Error> {
        let consumer: StreamConsumer = self.create_consumer().await?;

        let maybe_last_offset = last_kafka_offset_processed(&self.pool, H::TOPIC)
            .await
            .with_context(|| {
                format!(
                    "Failed retrieve last event processed for Kafka topic {}",
                    H::TOPIC
                )
            })?;

        loop {
            let message = consumer.recv().await.map_err(|e| anyhow!(e))?;
            let offset = message.offset();

            // Bypass any messages we think we've processed before.
            if let Some(last_offset) = maybe_last_offset {
                if last_offset >= offset {
                    warn!(
                        "Bypassing message {offset} for topic {}. Last message processed {last_offset}.",
                        H::TOPIC
                    );
                    consumer
                        .commit_message(&message, CommitMode::Async)
                        .map_err(anyhow::Error::new)?;
                    continue;
                }
            }

            if let Some(payload) = message.payload() {
                if let Ok(external_message) =
                    serde_json::from_slice::<<H as KafkaMessageHandler>::Message>(payload)
                {
                    self.handler.handle_message(offset, external_message).await;
                    update_last_kafka_offset_processed(&self.pool, H::TOPIC, offset).await?;
                } else {
                    error!(
                        "Could not deserialize Kafka message: Topic: {} Offset: {}. Bypassing message.",
                        H::TOPIC,
                        message.offset()
                    );
                    update_last_kafka_offset_processed(&self.pool, H::TOPIC, offset).await?;
                }
            } else {
                warn!(
                    "Ignoring Kafka message without payload: Topic: {} Offset: {}.",
                    H::TOPIC,
                    message.offset()
                );
                update_last_kafka_offset_processed(&self.pool, H::TOPIC, message.offset()).await?;
            }

            consumer
                .commit_message(&message, CommitMode::Async)
                .map_err(anyhow::Error::new)?;
        }
    }

    async fn create_consumer(&self) -> Result<StreamConsumer, anyhow::Error> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", H::GROUP)
            .set("bootstrap.servers", &self.settings.bootstrap_servers)
            .set("enable.partition.eof", "false")
            .set(
                "session.timeout.ms",
                self.settings.session_timeout_ms.to_string(),
            )
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "earliest")
            .create()
            .with_context(|| format!("Could not create StreamConsumer for group {}", H::GROUP))?;

        consumer
            .subscribe(&[H::TOPIC])
            .with_context(|| format!("Could not subscribe StreamConsumer to topic {}", H::TOPIC))?;

        Ok(consumer)
    }
}

#[async_trait]
impl<H> IntoSubsystem<anyhow::Error> for KafkaListener<H>
where
    H: KafkaMessageHandler + Clone + Send + Sync + 'static,
    for<'de> <H as KafkaMessageHandler>::Message: serde::Deserialize<'de>,
{
    async fn run(self, subsys: SubsystemHandle) -> Result<(), anyhow::Error> {
        select!(
            _ = self.listen() => {
                // TODO Improve error handling.
            }
            _ = subsys.on_shutdown_requested().map(Ok::<(), anyhow::Error>) => {
                info!("Kafka message handler for topic {} shutdown", H::TOPIC);
            }
        );
        Ok(())
    }
}

//----------------------------- SQL -----------------------------

/// Fetches the last Kafla event offest that was successfully processed for a topic.
async fn last_kafka_offset_processed(
    pool: &PgPool,
    topic: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT last_offset
           from kafka_topic
           where topic = $1;"#,
        topic
    )
    .fetch_optional(pool)
    .await
}

/// Records the last Kafla event offest that has been successfully processed for a topic.
async fn update_last_kafka_offset_processed(
    pool: &PgPool,
    topic: &str,
    last_event_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO kafka_topic (topic, last_offset)
           VALUES ($1, $2)
           ON CONFLICT(topic)
               DO UPDATE SET
                   last_offset = $2
                   WHERE kafka_topic.last_offset < $2;"#,
        topic,
        last_event_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

//---------------------------- Tests -----------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use fake::{Fake, Faker};
    use sqlx::PgPool;

    #[sqlx::test]
    async fn topic_offset_updates_can_be_found(pool: PgPool) {
        let test_cnt: usize = 10;
        let topic_offsets: Vec<(String, i64)> = (0..test_cnt)
            .map(|_| (Faker.fake(), Faker.fake()))
            .collect();

        for (topic, offset) in &topic_offsets {
            update_last_kafka_offset_processed(&pool, topic, *offset)
                .await
                .unwrap_or_else(|_| {
                    panic!("Expected to update topic: {topic} to offset {offset}.")
                });
        }

        for (topic, expected_offset) in &topic_offsets {
            let offset = last_kafka_offset_processed(&pool, topic)
                .await
                .unwrap_or_else(|_| panic!("Expected query to work."))
                .unwrap_or_else(|| panic!("Expected to find topic: {topic}."));
            assert_eq!(
                offset, *expected_offset,
                "Offset for topic {} is wrong.",
                topic
            );
        }
    }
}
