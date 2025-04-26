use anyhow::Context;
use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
};
use disintegrate::Decision;
use rust_decimal::Decimal;
use tracing::error;
use uuid::Uuid;

use crate::{
    domain::{DecisionMaker, DomainEvent, helpers::Stateless},
    infra::ClientError,
    subsystems::KafkaMessageHandler,
};

use super::{CartError, ProductId};

//------------------------- Web API ----------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChangePricePayload {
    pub product_id: Uuid,
    pub new_price: Decimal,
    pub old_price: Decimal,
}

pub async fn change_price_endpoint(
    State(decider): State<DecisionMaker>,
    Path(product_id): Path<Uuid>,
    Json(payload): Json<ChangePricePayload>,
) -> Result<Json<(Uuid, i64)>, ClientError> {
    if product_id != payload.product_id {
        return Err(ClientError::Payload(
            "Path PrpductId does not match payload ProductId.".to_owned(),
        ));
    }

    let decision: ChangePriceCommand = payload.try_into()?;
    let events = decider.make(decision).await?;

    let last_event_id = events
        .into_iter()
        .last()
        .map(|e| e.id())
        .context("No event returned for AddItemCommand!")?;

    Ok(Json((product_id, last_event_id)))
}

//------------------------- Command ----------------------------

/// The command used for processing ExternalPriceChanged events/messages.
#[derive(Debug, Clone)]
pub struct ChangePriceCommand {
    pub product_id: ProductId,
    pub old_price: Decimal,
    pub new_price: Decimal,
}

impl Decision for ChangePriceCommand {
    type Event = DomainEvent;
    type StateQuery = Stateless;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        Stateless
    }

    fn process(&self, _state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        Ok(vec![DomainEvent::PriceChanged {
            product_id: self.product_id,
            old_price: self.old_price,
            new_price: self.new_price,
        }])
    }
}

impl TryFrom<ChangePricePayload> for ChangePriceCommand {
    type Error = ClientError;

    fn try_from(payload: ChangePricePayload) -> Result<Self, Self::Error> {
        let product_id = payload.product_id.try_into()?;
        Ok(ChangePriceCommand {
            product_id,
            old_price: payload.old_price,
            new_price: payload.new_price,
        })
    }
}

//--------------------------- Processor -----------------------------

/// An external message we receive from Kafka.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PriceChangedMessage {
    pub product_uuid: Uuid,
    pub old_price: Decimal,
    pub new_price: Decimal,
}

/// The handler that processes ExternalPriceChanged events/messages.
#[derive(Clone)]
pub struct PriceChangeTranslator {
    decider: DecisionMaker,
}

impl PriceChangeTranslator {
    pub fn new(decider: DecisionMaker) -> Self {
        Self { decider }
    }
}

#[async_trait]
impl KafkaMessageHandler for PriceChangeTranslator {
    type Message = PriceChangedMessage;
    const GROUP: &str = "cart";
    const TOPIC: &str = "price-changes";

    async fn handle_message(&self, _offset: i64, message: Self::Message) {
        match ProductId::try_from(message.product_uuid) {
            Ok(product_id) => {
                let decision = ChangePriceCommand {
                    product_id,
                    old_price: message.old_price,
                    new_price: message.new_price,
                };
                if let Err(error) = self.decider.make(decision).await {
                    error!("PriceChangeTranslator: ChangePriceCommand failed with {error}");
                }
            }
            Err(error) => {
                error!(
                    "PriceChangeTranslator: Failed to extract ProductId from message due to {error:?}"
                );
            }
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use crate::domain::helpers::fake::Price;

    use super::*;
    use disintegrate::TestHarness;
    use fake::Fake;

    #[test]
    fn price_changed_event_should_be_created() {
        let product_id = ProductId::new();
        let old_price = Price.fake();
        let new_price = Price.fake();

        TestHarness::given([])
            .when(ChangePriceCommand {
                product_id,
                old_price,
                new_price,
            })
            .then([DomainEvent::PriceChanged {
                product_id,
                old_price,
                new_price,
            }]);
    }
}
