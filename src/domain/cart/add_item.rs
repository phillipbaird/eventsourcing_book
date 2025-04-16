//! Add Item slice

use anyhow::Context;
use axum::Json;
use axum::extract::{Path, State};
use disintegrate::{Decision, StateMutate, StateQuery};
use rust_decimal::Decimal;
use std::path::PathBuf;
use uuid::Uuid;

use crate::domain::helpers::device_fingerprint_calculator::calculate_device_fingerprint;
use crate::domain::{CartStream, DecisionMaker, DomainEvent};
use crate::infra::ClientError;

use super::{CartError, CartId, ItemId, ProductId};

//------------------------- Web API ----------------------------

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AddItemPayload {
    pub cart_id: Uuid,
    pub description: String,
    pub image: String,
    pub price: Decimal,
    pub item_id: Uuid,
    pub product_id: Uuid,
}

pub async fn add_item_endpoint(
    State(decider): State<DecisionMaker>,
    Path(cart_id): Path<Uuid>,
    Json(payload): Json<AddItemPayload>,
) -> Result<Json<(Uuid, i64)>, ClientError> {
    if cart_id != payload.cart_id {
        return Err(ClientError::Payload(
            "Path CartId does not match payload CartId.".to_owned(),
        ));
    }

    let mut decision: AddItemCommand = payload.try_into()?;
    decision.fingerprint = calculate_device_fingerprint();

    let events = decider.make(decision).await?;

    let last_event_id = events
        .into_iter()
        .last()
        .map(|e| e.id())
        .context("No event returned for AddItemCommand!")?;

    Ok(Json((cart_id, last_event_id)))
}

//------------------------- Command ----------------------------

#[derive(Debug, Clone)]
pub struct AddItemCommand {
    pub cart_id: CartId,
    pub description: String,
    pub image: PathBuf,
    pub price: Decimal,
    pub item_id: ItemId,
    pub product_id: ProductId,
    pub fingerprint: String,
}

impl TryFrom<AddItemPayload> for AddItemCommand {
    type Error = ClientError;

    fn try_from(payload: AddItemPayload) -> Result<Self, Self::Error> {
        let cart_id = payload.cart_id.try_into()?;
        let item_id = payload.item_id.try_into()?;
        let product_id = payload.product_id.try_into()?;
        Ok(Self {
            cart_id,
            description: payload.description,
            image: payload.image.into(),
            price: payload.price,
            item_id,
            product_id,
            fingerprint: Default::default(),
        })
    }
}

impl Decision for AddItemCommand {
    type Event = DomainEvent;
    type StateQuery = AddItemState;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        AddItemState {
            cart_id: self.cart_id,
            cart_exists: false,
            item_count: 0,
            submitted: false,
        }
    }

    fn process(&self, state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        if state.submitted {
            return Err(CartError::CartCannotBeAltered);
        }

        if state.item_count >= 3 {
            return Err(CartError::CannotAddItemCartFull);
        }

        let mut events = Vec::<DomainEvent>::new();
        if !state.cart_exists {
            events.push(DomainEvent::CartCreated {
                cart_id: self.cart_id,
            });
        }

        events.push(DomainEvent::CartItemAdded {
            cart_id: self.cart_id,
            description: self.description.clone(),
            image: self.image.clone(),
            price: self.price,
            item_id: self.item_id,
            product_id: self.product_id,
            fingerprint: self.fingerprint.clone(),
        });

        Ok(events)
    }
}

//---------------------- Command State --------------------------

#[derive(Clone, Debug, PartialEq, Eq, StateQuery, serde::Serialize, serde::Deserialize)]
#[state_query(CartStream)]
pub struct AddItemState {
    #[id]
    cart_id: CartId,
    cart_exists: bool,
    item_count: u8,
    submitted: bool,
}

impl StateMutate for AddItemState {
    fn mutate(&mut self, event: Self::Event) {
        match event {
            CartStream::CartCreated { .. } => {
                self.cart_exists = true;
            }
            CartStream::CartItemAdded { .. } => {
                self.item_count += 1;
            }
            CartStream::CartItemRemoved { .. } => {
                self.item_count -= 1;
            }
            CartStream::CartCleared { .. } => {
                self.item_count = 0;
            }
            CartStream::ItemArchivedEvent { .. } => {
                self.item_count -= 1;
            }
            CartStream::CartSubmitted { .. } => {
                self.submitted = true;
            }
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {

    use crate::domain::fake::{FingerPrint, Price};

    use super::*;
    use disintegrate::TestHarness;
    use fake::{Fake, Faker};

    #[test]
    fn cart_and_item_should_be_added_if_cart_does_not_exist() {
        let cart_id = CartId::new();
        let description: String = Faker.fake();
        let image: PathBuf = Faker.fake();
        let price = Price.fake();
        let item_id = ItemId::new();
        let product_id = ProductId::new();
        let fingerprint: String = FingerPrint.fake();

        TestHarness::given([])
            .when(AddItemCommand {
                cart_id,
                description: description.clone(),
                image: image.clone(),
                price,
                item_id,
                product_id,
                fingerprint: fingerprint.clone(),
            })
            .then([
                DomainEvent::CartCreated { cart_id },
                DomainEvent::CartItemAdded {
                    cart_id,
                    description,
                    image,
                    price,
                    item_id,
                    product_id,
                    fingerprint,
                },
            ])
    }

    #[test]
    fn item_should_be_added_if_cart_exists_and_has_space() {
        let cart_id = CartId::new();
        let description: String = Faker.fake();
        let image: PathBuf = Faker.fake();
        let price = Faker.fake();
        let item_id = ItemId::new();
        let product_id = ProductId::new();
        let fingerprint: String = FingerPrint.fake();

        TestHarness::given([DomainEvent::CartCreated { cart_id }])
            .when(AddItemCommand {
                cart_id,
                description: description.clone(),
                image: image.clone(),
                price,
                item_id,
                product_id,
                fingerprint: fingerprint.clone()
            })
            .then([DomainEvent::CartItemAdded {
                cart_id,
                description,
                image,
                price,
                item_id,
                product_id,
                fingerprint,
            }])
    }

    #[test]
    fn item_should_not_be_added_if_cart_is_full() {
        let cart_id = CartId::new();
        TestHarness::given([
            DomainEvent::CartCreated { cart_id },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Faker.fake(),
                item_id: ItemId::new(),
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Faker.fake(),
                item_id: ItemId::new(),
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Faker.fake(),
                item_id: ItemId::new(),
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
        ])
        .when(AddItemCommand {
            cart_id,
            ..Faker.fake()
        })
        .then_err(CartError::CannotAddItemCartFull);
    }
}
