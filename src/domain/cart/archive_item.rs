use disintegrate::{Decision, StateMutate, StateQuery};
use sqlx::PgPool;
use tracing::error;

use crate::domain::{CartStream, DecisionMaker, DomainEvent};

use super::{CartError, CartId, ItemId, ProductId, carts_with_products::find_by_product_id};

//------------------------- Command ----------------------------

pub struct ArchiveItemCommand {
    pub cart_id: CartId,
    pub item_id: ItemId,
    pub price_changed_event_id: i64,
}

impl Decision for ArchiveItemCommand {
    type Event = DomainEvent;
    type StateQuery = ArchiveItemState;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        ArchiveItemState {
            cart_id: self.cart_id,
            item_id: self.item_id,
            cart_exists: false,
            item_exists: false,
            submitted: false,
        }
    }

    fn process(&self, state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        if state.submitted {
            return Err(CartError::CartCannotBeAltered);
        }

        if state.cart_exists && state.item_exists {
            Ok(vec![DomainEvent::ItemArchivedEvent {
                cart_id: self.cart_id,
                item_id: self.item_id,
                price_changed_event_id: self.price_changed_event_id,
            }])
        } else {
            Ok(Vec::new())
        }
    }
}

//---------------------- Command State --------------------------

#[derive(Clone, Debug, PartialEq, Eq, StateQuery, serde::Serialize, serde::Deserialize)]
#[state_query(CartStream)]
pub struct ArchiveItemState {
    #[id]
    cart_id: CartId,
    item_id: ItemId,
    cart_exists: bool,
    item_exists: bool,
    submitted: bool,
}

impl StateMutate for ArchiveItemState {
    fn mutate(&mut self, event: Self::Event) {
        match event {
            CartStream::CartCreated { .. } => {
                self.cart_exists = true;
            }
            CartStream::CartItemAdded { item_id, .. } => {
                if item_id == self.item_id {
                    self.item_exists = true;
                }
            }
            CartStream::CartItemRemoved { item_id, .. } => {
                if item_id == self.item_id {
                    self.item_exists = false;
                }
            }
            CartStream::CartCleared { .. } => {
                self.item_exists = false;
            }
            CartStream::ItemArchivedEvent { item_id, .. } => {
                if item_id == self.item_id {
                    self.item_exists = false;
                }
            }
            CartStream::CartSubmitted { .. } => {
                self.submitted = true;
            }
        }
    }
}


//--------------------------- Processor -----------------------------

pub async fn archive_product_processor(
    pool: &PgPool,
    decider: &DecisionMaker,
    product_id: ProductId,
    triggering_event_id: i64,
) {
    match find_by_product_id(pool, &product_id).await {
        Ok(cart_items) => {
            let mut error_count = 0;

            for cart_item in cart_items {
                let decision = ArchiveItemCommand {
                    cart_id: cart_item.cart_id,
                    item_id: cart_item.item_id,
                    price_changed_event_id: triggering_event_id,
                };
                if let Err(error) = decider.make(decision).await {
                    error!(
                        "ArchiveProductProcessor: ArchiveItemCommand failed for cart {} item {} with error: {error:?}",
                        cart_item.cart_id, cart_item.item_id,
                    );
                    error_count += 1;
                }
            }

            if error_count > 0 {
                error!(
                    "ArchiveProductProcessor: There were {error_count} errors archiving product {product_id}"
                );
            }
        }
        Err(error) => {
            error!(
                "ArchiveProductProcessor: CartsWithProductsReadModel failed for product ({product_id}). Failed with {error:?}"
            );
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use crate::domain::helpers::fake::FingerPrint;

    use super::*;
    use disintegrate::TestHarness;
    use fake::{Fake, Faker};

    #[test]
    fn item_should_be_archived_if_cart_exists_and_has_item() {
        let cart_id = CartId::new();
        let item_id = ItemId::new();

        TestHarness::given([
            DomainEvent::CartCreated { cart_id },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Faker.fake(),
                item_id,
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
        ])
        .when(ArchiveItemCommand {
            cart_id,
            item_id,
            price_changed_event_id: 10,
        })
        .then([DomainEvent::ItemArchivedEvent {
            cart_id,
            item_id,
            price_changed_event_id: 10,
        }])
    }

    #[test]
    fn nothing_should_happen_if_archive_has_been_processed_prevously() {
        let cart_id = CartId::new();
        let item_id = ItemId::new();

        TestHarness::given([
            DomainEvent::CartCreated { cart_id },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Faker.fake(),
                item_id,
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
            DomainEvent::ItemArchivedEvent {
                cart_id,
                item_id,
                price_changed_event_id: 1,
            },
        ])
        .when(ArchiveItemCommand {
            cart_id,
            item_id,
            price_changed_event_id: 1,
        })
        .then([])
    }

    #[test]
    fn nothing_should_happen_if_cart_does_not_exist() {
        let cart_id = CartId::new();
        TestHarness::given([])
            .when(ArchiveItemCommand {
                cart_id,
                item_id: ItemId::new(),
                price_changed_event_id: 1,
            })
            .then([]);
    }
}
