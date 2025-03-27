use std::time::Duration;

use async_trait::async_trait;
use disintegrate::{EventListener, PersistedEvent, StreamQuery, query};
use rust_decimal::Decimal;
use tracing::error;

use crate::{
    domain::{
        DomainEvent,
        events::SubmittedStream,
        helpers::{PublishError, publish_with_events},
    },
    infra::KafkaSettings,
    subsystems::work_queue::{TaskArgs, TaskDomainArgs, TaskLimit, TaskTrigger, WorkQueue},
};

use super::{CartId, ProductId};

//------------ Event Handler for triggering Processor -----------

pub struct CartSubmittedEventHandler {
    query: StreamQuery<i64, SubmittedStream>,
    queue: WorkQueue,
}

impl CartSubmittedEventHandler {
    pub fn new(queue: WorkQueue) -> Self {
        Self {
            queue,
            query: query!(SubmittedStream),
        }
    }
}

#[async_trait]
impl EventListener<i64, SubmittedStream> for CartSubmittedEventHandler {
    type Error = anyhow::Error;

    fn id(&self) -> &'static str {
        "cart_submitted"
    }

    fn query(&self) -> &StreamQuery<i64, SubmittedStream> {
        &self.query
    }

    async fn handle(&self, event: PersistedEvent<i64, SubmittedStream>) -> Result<(), Self::Error> {
        let event_id = event.id();
        match event.into_inner() {
            SubmittedStream::CartSubmitted {
                cart_id,
                ordered_product,
                total_price,
            } => {
                let task_args = TaskArgs {
                    trigger: TaskTrigger::Event(event_id),
                    limits: TaskLimit::TimeoutAfter(Duration::from_secs(3600)),
                    domain_args: TaskDomainArgs::PublishCart(PublishCartProcessorArgs {
                        triggering_event_id: event_id,
                        message: ExternalPublishCart {
                            cart_id,
                            ordered_product: ordered_product
                                .into_iter()
                                .map(|op| op.into())
                                .collect(),
                            total_price,
                        },
                    }),
                };

                self.queue
                    .push(task_args)
                    .await
                    .inspect_err(|e| error!("CartSubmittedEventHandler: Failed to queue PublishCart task for event {event_id} due to {e}."))?;
            }
        }

        Ok(())
    }
}
//---------------------- Processor  -----------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublishCartProcessorArgs {
    pub triggering_event_id: i64,
    pub message: ExternalPublishCart,
}

pub async fn publish_cart_processor(
    settings: &KafkaSettings,
    event_store: &crate::domain::EventStore,
    args: PublishCartProcessorArgs,
) -> Result<(), PublishError> {
    let success_event = DomainEvent::CartPublished {
        cart_id: args.message.cart_id,
    };
    publish_with_events(
        settings,
        event_store,
        "published-carts".to_string(),
        &args.message,
        success_event,
    )
    .await
}

#[derive(Debug, Clone, Hash, serde::Deserialize, serde::Serialize)]
pub struct ExternalPublishCart {
    pub cart_id: CartId,
    pub ordered_product: Vec<OrderedProduct>,
    pub total_price: Decimal,
}

#[derive(Debug, Clone, Hash, serde::Deserialize, serde::Serialize)]
pub struct OrderedProduct {
    pub product_id: ProductId,
    pub price: Decimal,
}

impl From<crate::domain::events::OrderedProduct> for OrderedProduct {
    fn from(value: crate::domain::events::OrderedProduct) -> Self {
        OrderedProduct {
            product_id: value.product_id,
            price: value.price,
        }
    }
}
