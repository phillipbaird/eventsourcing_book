//! Submit Cart slice.

use anyhow::Context;
use axum::Json;
use axum::extract::{Path, State};
use disintegrate::{Decision, StateMutate, StateQuery};
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

use crate::domain::events::OrderedProduct;
use crate::domain::{CartStream, DecisionMaker, DomainEvent};
use crate::infra::ClientError;

use super::{CartError, CartId, ItemId, ProductId};

//------------------------- Web API ----------------------------

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubmitCartPayload {
    pub cart_id: Uuid,
}

pub async fn submit_cart_endpoint(
    State(decider): State<DecisionMaker>,
    Path(cart_id): Path<Uuid>,
    Json(payload): Json<SubmitCartPayload>,
) -> Result<Json<(Uuid, i64)>, ClientError> {
    if cart_id != payload.cart_id {
        return Err(ClientError::Payload(
            "Path CartId does not match payload CartId.".to_owned(),
        ));
    }

    let decision: SubmitCartCommand = payload.try_into()?;
    let events = decider.make(decision).await?;

    let last_event_id = events
        .into_iter()
        .last()
        .map(|e| e.id())
        .context("No event returned for SubmitCartCommand!")?;

    Ok(Json((cart_id, last_event_id)))
}

//------------------------- Command ----------------------------

#[derive(Debug, Clone)]
pub struct SubmitCartCommand {
    pub cart_id: CartId,
}

impl TryFrom<SubmitCartPayload> for SubmitCartCommand {
    type Error = ClientError;

    fn try_from(payload: SubmitCartPayload) -> Result<Self, Self::Error> {
        let cart_id = payload.cart_id.try_into()?;
        Ok(Self { cart_id })
    }
}

impl Decision for SubmitCartCommand {
    type Event = DomainEvent;
    type StateQuery = SubmitCartState;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        SubmitCartState {
            cart_id: self.cart_id,
            cart_exists: false,
            item_count: 0,
            submitted: false,
            cart_items: HashMap::new(),
            product_price: HashMap::new(),
        }
    }

    fn process(&self, state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        if !state.cart_exists {
            return Err(CartError::CartDoesNotExist(self.cart_id));
        }
        if state.item_count == 0 {
            return Err(CartError::CannotSubmitEmptyCart);
        }
        if state.submitted {
            return Err(CartError::CannotSubmitCartTwice);
        }

        let ordered_product: Vec<_> = state
            .cart_items
            .values()
            .map(|product_id| OrderedProduct {
                product_id: *product_id,
                price: state.product_price[product_id],
            })
            .collect();
        let total_price = ordered_product
            .iter()
            .map(|OrderedProduct { price, .. }| price)
            .sum();

        Ok(vec![
            (DomainEvent::CartSubmitted {
                cart_id: self.cart_id,
                ordered_product,
                total_price,
            }),
        ])
    }
}

//---------------------- Command State --------------------------

#[derive(Clone, Debug, PartialEq, Eq, StateQuery, serde::Serialize, serde::Deserialize)]
#[state_query(CartStream)]
pub struct SubmitCartState {
    #[id]
    cart_id: CartId,
    cart_exists: bool,
    item_count: u8,
    submitted: bool,
    cart_items: HashMap<ItemId, ProductId>,
    product_price: HashMap<ProductId, Decimal>,
}

impl StateMutate for SubmitCartState {
    fn mutate(&mut self, event: Self::Event) {
        match event {
            CartStream::CartCreated { .. } => {
                self.cart_exists = true;
            }
            CartStream::CartItemAdded {
                item_id,
                product_id,
                price,
                ..
            } => {
                self.item_count += 1;
                self.cart_items.insert(item_id, product_id);
                self.product_price.insert(product_id, price);
            }
            CartStream::CartItemRemoved { item_id, .. } => {
                self.item_count -= 1;
                let product_id = self.cart_items[&item_id];
                self.product_price.remove(&product_id);
                self.cart_items.remove(&item_id);
            }
            CartStream::CartCleared { .. } => {
                self.item_count = 0;
                self.cart_items.clear();
                self.product_price.clear();
            }
            CartStream::ItemArchivedEvent { item_id, .. } => {
                self.item_count -= 1;
                let product_id = self.cart_items[&item_id];
                self.product_price.remove(&product_id);
                self.cart_items.remove(&item_id);
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
    use crate::domain::helpers::{device_fingerprint_calculator::default_fingerprint, fake::Price};

    use super::*;
    use disintegrate::TestHarness;
    use fake::{Fake, Faker};

    #[test]
    fn should_not_submit_if_cart_does_not_exist() {
        let cart_id = CartId::new();

        TestHarness::given([])
            .when(SubmitCartCommand { cart_id })
            .then_err(CartError::CartDoesNotExist(cart_id))
    }

    #[test]
    fn should_not_submit_if_cart_is_empty() {
        let cart_id = CartId::new();

        TestHarness::given([DomainEvent::CartCreated { cart_id }])
            .when(SubmitCartCommand { cart_id })
            .then_err(CartError::CannotSubmitEmptyCart)
    }

    #[test]
    fn cart_should_be_submitted() {
        let cart_id = CartId::new();
        let product_id = ProductId::new();
        let price = Price.fake();
        TestHarness::given([
            DomainEvent::CartCreated { cart_id },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price,
                item_id: ItemId::new(),
                product_id,
                fingerprint: default_fingerprint(),
            },
        ])
        .when(SubmitCartCommand { cart_id })
        .then(vec![DomainEvent::CartSubmitted {
            cart_id,
            ordered_product: vec![OrderedProduct { product_id, price }],
            total_price: price,
        }]);
    }

    #[test]
    fn cart_should_not_be_submitted_twice() {
        let cart_id = CartId::new();
        let product_id = ProductId::new();
        let price = Price.fake();
        TestHarness::given([
            DomainEvent::CartCreated { cart_id },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price,
                item_id: ItemId::new(),
                product_id,
                fingerprint: default_fingerprint(),
            },
            DomainEvent::CartSubmitted {
                cart_id,
                ordered_product: vec![OrderedProduct { product_id, price }],
                total_price: price,
            },
        ])
        .when(SubmitCartCommand { cart_id })
        .then_err(CartError::CannotSubmitCartTwice);
    }
}
