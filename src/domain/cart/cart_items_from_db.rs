//! Cart Items read model converted to a database projection. See chapter 28.

use anyhow::Context;
use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use disintegrate::{query, EventListener, PersistedEvent, StreamQuery};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::CartStream, infra::ClientError};

use super::{CartError, CartId, CartItem, CartItemsReadModel, ItemId, ProductId};

//------------------------- Web API ----------------------------

pub async fn cart_items_from_db_endpoint(
    State(pool): State<PgPool>,
    Path(cart_uuid): Path<Uuid>,
) -> Result<Json<CartItemsReadModel>, ClientError> {
    let cart_id: CartId = cart_uuid.try_into()?;
    match cart_items_from_db_read_model(&pool, &cart_id).await {
        Ok(Some(read_model)) => Ok(Json(read_model)),
        Ok(None) => Err(CartError::CartDoesNotExist(cart_id).into()),
        Err(e) => Err(e.into()),
    }
}

//----------------------- Implementation --------------------------

pub async fn cart_items_from_db_read_model(
    pool: &PgPool,
    cart_id: &CartId,
) -> Result<Option<CartItemsReadModel>, anyhow::Error> {
    let maybe_uuid = sqlx::query_scalar!(r#"select cart_id from cart where cart_id = $1"#, cart_id as &CartId)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("Problem in cart_items_from_db_read_model({cart_id}) reading cart table."))?;
    if maybe_uuid.is_none() {
        return Ok(None);
    }

    let data = sqlx::query_as!(
        CartItem,
        r#"SELECT 
           cart_id as "cart_id: _",
           description,
           image,
           price,
           item_id as "item_id: _",
           product_id as "product_id: _",
           fingerprint
           from cart_items 
           where cart_id = $1;"#,
        &cart_id as &CartId
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("Problem in cart_items_from db_read_model{cart_id})"))?;

    let total_price: Decimal = data.iter().map(|i| i.price).sum();

    Ok(Some(CartItemsReadModel {
        cart_id: *cart_id,
        total_price,
        data,
    }))
}


pub async fn cart_items_from_db_read_model_reset(pool: &PgPool) -> Result<(), anyhow::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query!("DELETE FROM cart_items;")
        .execute(&mut *tx)
        .await
        .context("Problem in cart_items_from_db_read_model_reset.")?;

    sqlx::query("update event_listener set last_processed_event_id = 0 where id = $1;")
        .bind(PROJECTION_ID)
        .execute(&mut *tx)
        .await
        .context("Problem in cart_items_from_db_read_model_reset.")?;

   tx.commit().await
        .context("Problem in cart_items_from_db_read_model_reset.")
}

//------------------------- Projection --------------------------

const PROJECTION_ID: &str = "cart_items_from_db";

#[derive(Clone)]
pub struct CartItemsReadModelProjection {
    pool: PgPool,
    query: StreamQuery<i64, CartStream>,
}

impl CartItemsReadModelProjection {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            query: query!(CartStream),
        }
    }
}

#[async_trait]
impl EventListener<i64, CartStream> for CartItemsReadModelProjection {
    type Error = anyhow::Error;

    fn id(&self) -> &'static str {
        PROJECTION_ID
    }

    fn query(&self) -> &StreamQuery<i64, CartStream> {
        &self.query
    }

    async fn handle(&self, event: PersistedEvent<i64, CartStream>) -> Result<(), Self::Error> {
        let last_event_id = event.id();
        let event = event.into_inner();
        match event {
            CartStream::CartCleared { cart_id } => delete_by_cart_id(&self.pool, &cart_id, last_event_id).await,
            CartStream::CartCreated { cart_id } => add_cart(&self.pool, &cart_id).await,
            CartStream::CartItemAdded { cart_id, description, image, price, item_id, product_id, fingerprint } =>
                save(&self.pool, &cart_id, &description, image.to_string_lossy().as_ref(), price, &item_id, &product_id, &fingerprint, last_event_id).await,
            CartStream::CartItemRemoved { cart_id, item_id } => delete_by_item_id(&self.pool, &cart_id, &item_id, last_event_id).await,
            CartStream::CartSubmitted { .. } => Ok(()),
            CartStream::ItemArchivedEvent { cart_id, item_id, .. } => delete_by_item_id(&self.pool, &cart_id, &item_id, last_event_id).await,
        }
    }
}

//--------------------------- SQL -------------------------------

async fn add_cart(pool: &PgPool, cart_id: &CartId) -> Result<(), anyhow::Error> {
    sqlx::query!("INSERT INTO cart (cart_id) VALUES ($1)", cart_id as &CartId)
        .execute(pool)
        .await
        .with_context(|| format!("Problem in add_cart({cart_id})"))?;
    Ok(())
}

async fn delete_by_cart_id(
    pool: &PgPool,
    cart_id: &CartId,
    last_event_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"DELETE FROM cart_items
           WHERE cart_id = $1 and last_event_id < $2"#,
        cart_id as &CartId,
        last_event_id
    )
    .execute(pool)
    .await
    .with_context(|| {
        format!("Problem in delete_by_cart_id(cart_id: {cart_id}, last_event_id: {last_event_id}).")
    })?;
    Ok(())
}

async fn delete_by_item_id(
    pool: &PgPool,
    cart_id: &CartId,
    item_id: &ItemId,
    last_event_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"DELETE FROM cart_items
           WHERE cart_id = $1 and item_id = $2 and last_event_id < $3"#,
        cart_id as &CartId,
        item_id as &ItemId,
        last_event_id
    )
    .execute(pool)
    .await
    .with_context(|| format!("Problem in delete_by_item_id(cart_id: {cart_id}, item_id: {item_id}, last_event_id: {last_event_id})."))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn save(
    pool: &PgPool,
    cart_id: &CartId,
    description: &str,
    image: &str,
    price: Decimal,
    item_id: &ItemId,
    product_id: &ProductId,
    fingerprint: &str,
    last_event_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"INSERT INTO cart_items (cart_id, description, image, price, item_id, product_id, fingerprint, last_event_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           ON CONFLICT(cart_id, item_id)
           DO UPDATE SET
              description = $2,
              image = $3,
              price = $4,
              product_id = $6,
              fingerprint = $7,
              last_event_id = $8
              WHERE cart_items.last_event_id < $8"#,
        cart_id as &CartId,
        description,
        image,
        price,
        item_id as &ItemId,
        product_id as &ProductId,
        fingerprint,
        last_event_id
    )
    .execute(pool)
    .await
    .with_context(|| format!("Problem in save(cart_id: {cart_id}, item_id: {item_id}, product_id: {product_id}, last_event_id: {last_event_id})."))?;
    Ok(())
}



//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use crate::domain::{
        cart::{AddItemCommand, RemoveItemCommand},
        create_eventstore_and_decider, DomainEvent,
    };

    use super::*;

    use fake::{Fake, Faker};
    use rust_decimal::Decimal;
    use sqlx::PgPool;

    fn fake_price() -> Decimal {
        Decimal::new((1..1000).fake(), 2)
    }
    
    #[sqlx::test]
    async fn cart_items_read_model_test(pool: PgPool) {
        let (_, decider) = create_eventstore_and_decider(&pool)
            .await
            .expect("EventStore and Decider should be created.");

        let cart_id = CartId::new();

        let mut persisted_events: Vec<PersistedEvent<i64, DomainEvent>> = Vec::new();

        let price1 = fake_price();
        let add_item1_cmd = AddItemCommand {
            cart_id,
            price: price1,
            ..Faker.fake()
        };
        persisted_events.extend(decider
            .make(add_item1_cmd.clone())
            .await
            .expect("Add item 1 should succeed."));

        let item_id_to_remove = ItemId::new();
        let add_item2_cmd = AddItemCommand {
            cart_id,
            item_id: item_id_to_remove,
            ..Faker.fake()
        };
        persisted_events.extend(decider
            .make(add_item2_cmd)
            .await
            .expect("Add item 2 should succeed."));

        let price3 = fake_price();
        let add_item3_cmd = AddItemCommand {
            cart_id,
            price: price3,
            ..Faker.fake()
        };
        persisted_events.extend(decider
            .make(add_item3_cmd.clone())
            .await
            .expect("Add item 3 should succeed."));

        let remove_item2_cmd = RemoveItemCommand {
            cart_id,
            item_id: item_id_to_remove,
        };
        persisted_events.extend(decider
            .make(remove_item2_cmd)
            .await
            .expect("Removing item 2 should succeed."));

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

        let projection = CartItemsReadModelProjection::new(pool.clone());
        // Process the events through the projection.
        let persisted_events: Vec<PersistedEvent<i64, CartStream>> = 
            persisted_events.into_iter().map(|pe| PersistedEvent::new(
                    pe.id(), pe.into_inner().try_into().expect("Expected type converion.")
            )).collect();
        for event in persisted_events {
            projection
                .handle(event)
                .await
                .expect("Event should be handled.");
        }

        let read_model = cart_items_from_db_read_model(&pool, &cart_id)
            .await
            .expect("Cart Items readmodel should have been read.");

        pool.close().await;

        assert_eq!(read_model, expected_read_model);
    }

}
