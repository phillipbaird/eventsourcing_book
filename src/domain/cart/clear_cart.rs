//! Clear Cart slice

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

use super::{CartError, CartId};

//------------------------- Web API ----------------------------

pub async fn clear_cart_endpoint(
    State(decider): State<DecisionMaker>,
    Path(cart_uuid): Path<Uuid>,
) -> Result<Json<(Uuid, i64)>, ClientError> {
    let cart_id = cart_uuid.try_into()?;
    let decision = ClearCartCommand { cart_id };
    let events = decider.make(decision).await?;

    let last_event_id = events
        .into_iter()
        .last()
        .map(|e| e.id())
        .context("No event returned for ClearCartCommand!")?;

    Ok(Json((cart_uuid, last_event_id)))
}

//------------------------- Command ----------------------------

#[derive(Debug, Clone)]
pub struct ClearCartCommand {
    pub cart_id: CartId,
}

impl Decision for ClearCartCommand {
    type Event = DomainEvent;
    type StateQuery = ClearCartState;
    type Error = CartError;

    fn state_query(&self) -> Self::StateQuery {
        ClearCartState {
            cart_id: self.cart_id,
            cart_exists: false,
            submitted: false,
        }
    }

    fn process(&self, state: &Self::StateQuery) -> Result<Vec<Self::Event>, Self::Error> {
        if state.submitted {
            return Err(CartError::CartCannotBeAltered);
        }

        if !state.cart_exists {
            return Err(CartError::CartDoesNotExist(self.cart_id));
        }

        Ok(vec![DomainEvent::CartCleared {
            cart_id: self.cart_id,
        }])
    }
}

//---------------------- Command State --------------------------

#[derive(Clone, Debug, PartialEq, Eq, StateQuery, serde::Serialize, serde::Deserialize)]
#[state_query(CartStream)]
pub struct ClearCartState {
    #[id]
    cart_id: CartId,
    cart_exists: bool,
    submitted: bool,
}

impl StateMutate for ClearCartState {
    fn mutate(&mut self, event: Self::Event) {
        match event {
            CartStream::CartCreated { .. } => self.cart_exists = true,
            CartStream::CartCleared { .. } => {}
            CartStream::CartItemAdded { .. } => {}
            CartStream::CartItemRemoved { .. } => {}
            CartStream::CartSubmitted { .. } => self.submitted = true,
            CartStream::ItemArchivedEvent { .. } => {}
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use crate::domain::{
        cart::{ItemId, ProductId},
        helpers::fake::FingerPrint,
    };

    use super::*;
    use disintegrate::TestHarness;
    use fake::{Fake, Faker};

    #[test]
    fn cart_should_be_cleared_if_cart_exists() {
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
        ])
        .when(ClearCartCommand { cart_id })
        .then([DomainEvent::CartCleared { cart_id }])
    }

    #[test]
    fn should_error_if_cart_does_not_exist() {
        let cart_id = CartId::new();
        TestHarness::given([])
            .when(ClearCartCommand { cart_id })
            .then_err(CartError::CartDoesNotExist(cart_id));
    }
}
