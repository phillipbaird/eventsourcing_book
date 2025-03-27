use async_trait::async_trait;
use disintegrate::Decision;
use rust_decimal::Decimal;
use tracing::error;
use uuid::Uuid;

use crate::{
    domain::{DecisionMaker, DomainEvent, helpers::Stateless},
    subsystems::KafkaMessageHandler,
};

use super::{CartError, ProductId};

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

    async fn handle_message(&self, _offset: i64, event: Self::Message) {
        match ProductId::try_from(event.product_uuid) {
            Ok(product_id) => {
                let decision = ChangePriceCommand {
                    product_id,
                    old_price: event.old_price,
                    new_price: event.new_price,
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
