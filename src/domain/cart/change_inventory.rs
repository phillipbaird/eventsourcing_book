use async_trait::async_trait;
use disintegrate::Decision;
use tracing::error;
use uuid::Uuid;

use crate::{
    domain::{DecisionMaker, DomainEvent, helpers::Stateless},
    subsystems::KafkaMessageHandler,
};

use super::{CartError, ProductId};

// --------------------------- Command ----------------------------

#[derive(Debug, Clone)]
pub struct ChangeInventoryCommand {
    pub product_id: ProductId,
    pub inventory: i32,
}

impl Decision for ChangeInventoryCommand {
    type Event = DomainEvent;
    type StateQuery = Stateless;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        Stateless
    }

    fn process(&self, _state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        Ok(vec![DomainEvent::InventoryChanged {
            product_id: self.product_id,
            inventory: self.inventory,
        }])
    }
}

/// An external message we receive from Kafka.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct InventoryChangedMessage {
    pub product_uuid: Uuid,
    pub inventory: i32,
}

//--------------------------- Processor -----------------------------

#[derive(Clone)]
pub struct InventoryChangedTranslator {
    decider: DecisionMaker,
}

impl InventoryChangedTranslator {
    pub fn new(decider: DecisionMaker) -> Self {
        Self { decider }
    }
}

#[async_trait]
impl KafkaMessageHandler for InventoryChangedTranslator {
    type Message = InventoryChangedMessage;
    const GROUP: &str = "cart";
    const TOPIC: &str = "inventories";

    async fn handle_message(&self, _offset: i64, event: Self::Message) {
        match ProductId::try_from(event.product_uuid) {
            Ok(product_id) => {
                let decision = ChangeInventoryCommand {
                    product_id,
                    inventory: event.inventory,
                };
                if let Err(error) = self.decider.make(decision).await {
                    error!(
                        "InventoryChangedTranslator: ChangeInventoryCommand failed with {error}"
                    );
                }
            }
            Err(error) => {
                error!(
                    "InventoryChangedTranslator: Failed to extract ProductId from message due to {error}"
                );
            }
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use disintegrate::TestHarness;
    use fake::{Fake, Faker};

    #[test]
    fn inventory_changed_event_should_be_created() {
        let product_id = ProductId::new();
        let inventory: i32 = Faker.fake();

        TestHarness::given([])
            .when(ChangeInventoryCommand {
                product_id,
                inventory,
            })
            .then([DomainEvent::InventoryChanged {
                product_id,
                inventory,
            }]);
    }
}
