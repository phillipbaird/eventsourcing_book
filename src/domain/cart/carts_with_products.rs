//! CartsWithProducts read model

use anyhow::Context;
use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
};
use disintegrate::{EventListener, PersistedEvent, StreamQuery, query};
use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

use crate::{
    domain::{CartStream, DecisionMaker, DomainEvent, PricingStream},
    infra::ClientError,
};

use super::{CartId, ItemId, ProductId, archive_item::archive_product_processor};

//------------------------- Web API ----------------------------

pub async fn carts_with_products_endpoint(
    State(pool): State<PgPool>,
    Path(product_uuid): Path<Uuid>,
) -> Result<Json<Vec<CartsWithProductsReadModel>>, ClientError> {
    let product_id: ProductId = product_uuid.try_into()?;
    match find_by_product_id(&pool, &product_id).await {
        Ok(read_model) => Ok(Json(read_model)),
        Err(e) => Err(e.into()),
    }
}

//----------------------- Read Model API ------------------------

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CartsWithProductsReadModel {
    pub cart_id: CartId,
    pub item_id: ItemId,
    pub product_id: ProductId,
}

pub async fn find_by_product_id(
    pool: &PgPool,
    product_id: &ProductId,
) -> Result<Vec<CartsWithProductsReadModel>, anyhow::Error> {
    sqlx::query_as!(
        CartsWithProductsReadModel,
        r#"SELECT 
           cart_id as "cart_id: _",
           item_id as "item_id: _",
           product_id as "product_id: _" 
           from carts_with_products 
           where product_id = $1;"#,
        &product_id as &ProductId
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("Problem in find_by_product_id({product_id})"))
}

//------------------------- Projection --------------------------

#[derive(Clone)]
pub(crate) struct CartsWithProductsReadModelProjection {
    pool: PgPool,
    decider: DecisionMaker,
    query: StreamQuery<i64, DomainEvent>,
}

impl CartsWithProductsReadModelProjection {
    pub fn new(pool: PgPool, decider: DecisionMaker) -> Self {
        Self {
            pool,
            decider,
            query: query!(CartStream).union(&query!(PricingStream)),
        }
    }
}

#[async_trait]
impl EventListener<i64, DomainEvent> for CartsWithProductsReadModelProjection {
    type Error = anyhow::Error;

    fn id(&self) -> &'static str {
        "carts_with_products"
    }

    fn query(&self) -> &StreamQuery<i64, DomainEvent> {
        &self.query
    }

    async fn handle(&self, event: PersistedEvent<i64, DomainEvent>) -> Result<(), Self::Error> {
        let last_event_id = event.id();
        let event = event.into_inner();
        match event {
            DomainEvent::CartCreated { .. } => Ok(()),
            DomainEvent::CartItemAdded {
                cart_id,
                item_id,
                product_id,
                ..
            } => save(&self.pool, &cart_id, &item_id, &product_id, last_event_id).await,
            DomainEvent::CartItemRemoved { cart_id, item_id } =>
                delete_by_item_id(&self.pool, &cart_id, &item_id, last_event_id).await,
            DomainEvent::CartCleared { cart_id } =>
                delete_by_cart_id(&self.pool, &cart_id, last_event_id).await,
            DomainEvent::ItemArchivedEvent {
                cart_id, item_id, ..
            } => delete_by_item_id(&self.pool, &cart_id, &item_id, last_event_id).await,
            DomainEvent::PriceChanged { product_id, .. } => {
                archive_product_processor(&self.pool, &self.decider, product_id, last_event_id).await;
                Ok(())
            }
            DomainEvent::CartSubmitted { cart_id, .. } =>
                delete_by_cart_id(&self.pool, &cart_id, last_event_id).await,
            unexpected => {
                panic!("CartsWithProducts projection received unsupported event type {unexpected:?} for event {last_event_id}.")
            }
        }
        .inspect_err(|e| error!("CartsWithProductsReadModelProjection: Failed handling event ({last_event_id})\n{event:?}\nfailed with {e}"))
    }
}

//--------------------------- SQL -------------------------------

async fn delete_by_cart_id(
    pool: &PgPool,
    cart_id: &CartId,
    last_event_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"DELETE FROM carts_with_products
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
        r#"DELETE FROM carts_with_products
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

async fn save(
    pool: &PgPool,
    cart_id: &CartId,
    item_id: &ItemId,
    product_id: &ProductId,
    last_event_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query!(
        r#"INSERT INTO carts_with_products (cart_id, item_id, product_id, last_event_id)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT(cart_id, item_id, product_id)
           DO UPDATE SET
              last_event_id = $4
              WHERE carts_with_products.last_event_id < $4"#,
        cart_id as &CartId,
        item_id as &ItemId,
        product_id as &ProductId,
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
        create_eventstore_and_decider,
    };

    use super::*;
    use fake::{Fake, Faker};

    #[sqlx::test]
    async fn carts_with_products_read_model_test(pool: PgPool) {
        let (_event_store, decider) = create_eventstore_and_decider(&pool)
            .await
            .expect("Eventstore and DecisionMaker should be created.");
        let projection = CartsWithProductsReadModelProjection::new(pool.clone(), decider.clone());

        let cart_id = CartId::new();
        let product_id = ProductId::new();
        let item_id1 = ItemId::new();
        let item_id2 = ItemId::new();
        let item_id3 = ItemId::new();

        let commands = [
            AddItemCommand {
                cart_id,
                item_id: item_id1,
                product_id,
                ..Faker.fake()
            },
            AddItemCommand {
                cart_id,
                item_id: item_id2,
                product_id,
                ..Faker.fake()
            },
            AddItemCommand {
                cart_id,
                item_id: item_id3,
                product_id,
                ..Faker.fake()
            },
        ];

        let remove_item2_cmd = RemoveItemCommand {
            cart_id,
            item_id: item_id2,
        };

        // Process commands and collect the generated events.
        let mut persisted_events: Vec<PersistedEvent<i64, DomainEvent>> = Vec::new();
        for command in commands {
            persisted_events.extend(
                decider
                    .make(command)
                    .await
                    .expect("Command should be successful."),
            );
        }
        persisted_events.extend(
            decider
                .make(remove_item2_cmd)
                .await
                .expect("Removing item 2 should succeed."),
        );

        // Process the events through the projection.
        for event in persisted_events {
            projection
                .handle(event)
                .await
                .expect("Event should be handled.");
        }

        let expected_read_model = [
            CartsWithProductsReadModel {
                cart_id,
                item_id: item_id1,
                product_id,
            },
            CartsWithProductsReadModel {
                cart_id,
                item_id: item_id3,
                product_id,
            },
        ];

        let read_model = find_by_product_id(&pool, &product_id)
            .await
            .expect("CartsWithProductsReadModel should have been read.");

        pool.close().await;

        assert_eq!(read_model.len(), expected_read_model.len());
        assert!(read_model.contains(&expected_read_model[0]));
        assert!(read_model.contains(&expected_read_model[1]));
    }
}
