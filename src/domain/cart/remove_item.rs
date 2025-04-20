use anyhow::Context;
use axum::{
    Json,
    extract::{Path, State},
};
use disintegrate::{Decision, StateMutate, StateQuery};
use uuid::Uuid;

use crate::{
    domain::{CartStream, DecisionMaker, DomainEvent},
    infra::ClientError,
};

use super::{CartError, CartId, ItemId};

//------------------------- Web API ----------------------------

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RemoveItemPayload {
    pub cart_id: Uuid,
    pub item_id: Uuid,
}

pub async fn remove_item_endpoint(
    State(decider): State<DecisionMaker>,
    Path(cart_uuid): Path<Uuid>,
    Json(payload): Json<RemoveItemPayload>,
) -> Result<Json<(Uuid, i64)>, ClientError> {
    if cart_uuid != payload.cart_id {
        return Err(ClientError::Payload(
            "Path CartId does not match payload CartId.".to_owned(),
        ));
    }

    let decision: RemoveItemCommand = payload.try_into()?;
    let events = decider.make(decision).await?;

    let last_event_id = events
        .into_iter()
        .last()
        .map(|e| e.id())
        .context("No event returned for RemoveItemCommand!")?;

    Ok(Json((cart_uuid, last_event_id)))
}

//------------------------- Command ----------------------------

#[derive(Debug, Clone)]
pub struct RemoveItemCommand {
    pub cart_id: CartId,
    pub item_id: ItemId,
}

impl TryFrom<RemoveItemPayload> for RemoveItemCommand {
    type Error = ClientError;

    fn try_from(payload: RemoveItemPayload) -> Result<RemoveItemCommand, Self::Error> {
        let cart_id = payload.cart_id.try_into()?;
        let item_id = payload.item_id.try_into()?;
        Ok(RemoveItemCommand { cart_id, item_id })
    }
}

impl Decision for RemoveItemCommand {
    type Event = DomainEvent;
    type StateQuery = RemoveItemState;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        RemoveItemState {
            cart_id: self.cart_id,
            item_id: self.item_id,
            cart_exists: false,
            item_exists: false,
            submitted: false,
        }
    }

    fn process(&self, state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        if !state.cart_exists {
            return Err(CartError::CartDoesNotExist(self.cart_id));
        }

        if state.submitted {
            return Err(CartError::CartCannotBeAltered);
        }

        if !state.item_exists {
            return Err(CartError::CannotRemoveItem);
        }

        Ok(vec![DomainEvent::CartItemRemoved {
            cart_id: self.cart_id,
            item_id: self.item_id,
        }])
    }
}

//---------------------- Command State --------------------------

#[derive(Clone, Debug, PartialEq, Eq, StateQuery, serde::Serialize, serde::Deserialize)]
#[state_query(CartStream)]
pub struct RemoveItemState {
    #[id]
    cart_id: CartId,
    item_id: ItemId,
    cart_exists: bool,
    item_exists: bool,
    submitted: bool,
}

impl StateMutate for RemoveItemState {
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
            CartStream::CartSubmitted { .. } => self.submitted = true,
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use crate::domain::{
        cart::ProductId, helpers::device_fingerprint_calculator::default_fingerprint,
    };

    use super::*;
    use disintegrate::TestHarness;
    use fake::{Fake, Faker};

    #[test]
    fn item_should_be_removed_if_cart_exists_and_has_item() {
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
                fingerprint: default_fingerprint(),
            },
        ])
        .when(RemoveItemCommand { cart_id, item_id })
        .then([DomainEvent::CartItemRemoved { cart_id, item_id }])
    }

    #[test]
    fn item_should_not_be_removed_if_item_does_not_exist() {
        let cart_id = CartId::new();
        let item_id = ItemId::new();

        TestHarness::given([
            DomainEvent::CartCreated { cart_id },
            DomainEvent::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Faker.fake(),
                item_id: ItemId::new(),
                product_id: ProductId::new(),
                fingerprint: default_fingerprint(),
            },
        ])
        .when(RemoveItemCommand { cart_id, item_id })
        .then_err(CartError::CannotRemoveItem)
    }

    #[test]
    fn item_should_not_be_removed_if_cart_does_not_exist() {
        let cart_id = CartId::new();
        TestHarness::given([])
            .when(RemoveItemCommand {
                cart_id,
                item_id: ItemId::new(),
            })
            .then_err(CartError::CartDoesNotExist(cart_id));
    }
}
