//! Cart Items slice

use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use disintegrate::query;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    domain::{CartStream, EventReadingError, EventStore, read_from_events},
    infra::ClientError,
};

use super::{CartError, CartId, ItemId, ProductId};

//------------------------- Web API ----------------------------

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CartItemsReadModel {
    pub cart_id: CartId,
    pub total_price: Decimal,
    pub data: Vec<CartItem>,
}

impl CartItemsReadModel {
    pub fn new(cart_id: CartId) -> Self {
        Self {
            cart_id,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CartItem {
    pub cart_id: CartId,
    pub description: String,
    pub image: PathBuf,
    pub price: Decimal,
    pub item_id: ItemId,
    pub product_id: ProductId,
    pub fingerprint: String,
}

pub async fn cart_items_endpoint(
    State(event_store): State<EventStore>,
    Path(cart_uuid): Path<Uuid>,
) -> Result<Json<CartItemsReadModel>, ClientError> {
    let cart_id: CartId = cart_uuid.try_into()?;
    match cart_items_read_model(event_store, &cart_id).await {
        Ok(Some(read_model)) => Ok(Json(read_model)),
        Ok(None) => Err(CartError::CartDoesNotExist(cart_id).into()),
        Err(e) => Err(e.into()),
    }
}

//----------------------- Implementation --------------------------

pub async fn cart_items_read_model(
    event_store: EventStore,
    cart_id: &CartId,
) -> Result<Option<CartItemsReadModel>, EventReadingError<i64, disintegrate_postgres::Error>> {
    let query = query!(CartStream; cart_id == *cart_id);
    read_from_events(&event_store, &query, None, apply_event).await
}

fn apply_event(
    read_model: Option<CartItemsReadModel>,
    event: CartStream,
) -> Option<CartItemsReadModel> {
    match (read_model, event) {
        (None, CartStream::CartCreated { cart_id }) => Some(CartItemsReadModel::new(cart_id)),
        (
            Some(mut read_model),
            CartStream::CartItemAdded {
                cart_id,
                description,
                image,
                price,
                item_id,
                product_id,
                fingerprint,
            },
        ) => {
            read_model.total_price += price;
            read_model.data.push(CartItem {
                cart_id,
                description,
                image,
                price,
                item_id,
                product_id,
                fingerprint,
            });
            Some(read_model)
        }
        (Some(mut read_model), CartStream::CartItemRemoved { cart_id, item_id }) => {
            if let Some(item) = read_model.data.iter().find(|item| item.item_id == item_id) {
                read_model.total_price -= item.price;
            }
            read_model
                .data
                .retain(|item| item.cart_id == cart_id && item.item_id != item_id);
            Some(read_model)
        }
        (Some(mut read_model), CartStream::CartCleared { .. }) => {
            read_model.total_price = Decimal::default();
            read_model.data.clear();
            Some(read_model)
        }
        (
            Some(mut read_model),
            CartStream::ItemArchivedEvent {
                cart_id, item_id, ..
            },
        ) => {
            if let Some(item) = read_model.data.iter().find(|item| item.item_id == item_id) {
                read_model.total_price -= item.price;
            }
            read_model
                .data
                .retain(|item| item.cart_id == cart_id && item.item_id != item_id);
            Some(read_model)
        }
        (Some(read_model), CartStream::CartSubmitted { .. }) => Some(read_model),
        (None, _) => {
            panic!("The first event for the cart was not CartAdded! This should not happen.")
        }
        (Some(_), CartStream::CartCreated { .. }) => {
            panic!("A secondary event for the cart was CartCreated! This should not happen.")
        }
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use crate::domain::{
        cart::{AddItemCommand, RemoveItemCommand},
        create_eventstore_and_decider,
        helpers::fake::{FingerPrint, Price},
    };

    use super::*;

    use fake::{Fake, Faker};
    use sqlx::PgPool;

    fn cart_item_from_event(event: &CartStream) -> CartItem {
        if let CartStream::CartItemAdded {
            cart_id,
            description,
            image,
            price,
            item_id,
            product_id,
            fingerprint,
        } = event.clone()
        {
            CartItem {
                cart_id,
                description,
                image,
                price,
                item_id,
                product_id,
                fingerprint,
            }
        } else {
            panic!("Event not a CartItemAdded event!")
        }
    }

    #[test]
    fn given_events_then_cart_items_read_model() {
        // Given
        let cart_id = CartId::new();
        let item2_id = ItemId::new();
        let price1 = Price.fake();
        let price3 = Price.fake();
        let events = [
            CartStream::CartCreated { cart_id },
            CartStream::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: price1,
                item_id: ItemId::new(),
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
            CartStream::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: Price.fake(),
                item_id: item2_id,
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
            CartStream::CartItemAdded {
                cart_id,
                description: Faker.fake(),
                image: Faker.fake(),
                price: price3,
                item_id: ItemId::new(),
                product_id: ProductId::new(),
                fingerprint: FingerPrint.fake(),
            },
            CartStream::CartItemRemoved {
                cart_id,
                item_id: item2_id,
            },
        ];

        // Then
        let expected_read_model = Some(CartItemsReadModel {
            cart_id,
            total_price: price1 + price3,
            data: vec![
                cart_item_from_event(&events[1]),
                cart_item_from_event(&events[3]),
            ],
        });

        let read_model = events.into_iter().fold(None, apply_event);

        assert_eq!(read_model, expected_read_model);
    }

    #[sqlx::test]
    async fn cart_items_read_model_test(pool: PgPool) {
        let (event_store, decider) = create_eventstore_and_decider(&pool)
            .await
            .expect("EventStore and Decider should be created.");

        let cart_id = CartId::new();

        let price1 = Price.fake();
        let add_item1_cmd = AddItemCommand {
            cart_id,
            price: price1,
            ..Faker.fake()
        };
        decider
            .make(add_item1_cmd.clone())
            .await
            .expect("Add item 1 should succeed.");

        let item_id_to_remove = ItemId::new();
        let add_item2_cmd = AddItemCommand {
            cart_id,
            item_id: item_id_to_remove,
            ..Faker.fake()
        };
        decider
            .make(add_item2_cmd)
            .await
            .expect("Add item 2 should succeed.");

        let price3 = Price.fake();
        let add_item3_cmd = AddItemCommand {
            cart_id,
            price: price3,
            ..Faker.fake()
        };
        decider
            .make(add_item3_cmd.clone())
            .await
            .expect("Add item 3 should succeed.");

        let remove_item2_cmd = RemoveItemCommand {
            cart_id,
            item_id: item_id_to_remove,
        };
        decider
            .make(remove_item2_cmd)
            .await
            .expect("Removing item 2 should succeed.");

        let expected_read_model = Some(CartItemsReadModel {
            cart_id,
            total_price: price1 + price3,
            data: vec![
                CartItem {
                    cart_id: add_item1_cmd.cart_id,
                    description: add_item1_cmd.description,
                    image: add_item1_cmd.image,
                    price: add_item1_cmd.price,
                    item_id: add_item1_cmd.item_id,
                    product_id: add_item1_cmd.product_id,
                    fingerprint: add_item1_cmd.fingerprint,
                },
                CartItem {
                    cart_id: add_item3_cmd.cart_id,
                    description: add_item3_cmd.description,
                    image: add_item3_cmd.image,
                    price: add_item3_cmd.price,
                    item_id: add_item3_cmd.item_id,
                    product_id: add_item3_cmd.product_id,
                    fingerprint: add_item3_cmd.fingerprint,
                },
            ],
        });

        let read_model = cart_items_read_model(event_store, &cart_id)
            .await
            .expect("Cart Items readmodel should have been read.");

        pool.close().await;

        assert_eq!(read_model, expected_read_model);
    }
}
