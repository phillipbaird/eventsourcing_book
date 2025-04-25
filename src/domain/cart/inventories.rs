//! Inventories read model.
use anyhow::Context;
use async_trait::async_trait;
use axum::{extract::{Path, State}, Json};
use disintegrate::{EventListener, PersistedEvent, StreamQuery, query};
use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

use crate::{domain::InventoryStream, infra::ClientError};

use super::ProductId;

//------------------------- Web API ----------------------------

pub async fn inventories_endpoint(
    State(pool): State<PgPool>,
    Path(product_uuid): Path<Uuid>,
) -> Result<Json<Option<InventoriesReadModel>>, ClientError> {
    let product_id: ProductId = product_uuid.try_into()?;
    match find_by_id(&pool, &product_id).await {
        Ok(read_model) => Ok(Json(read_model)),
        Err(e) => Err(e.into()),
    }
}

//----------------------- Read Model API ------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct InventoriesReadModel {
    pub product_id: ProductId,
    pub inventory: i32,
}

pub async fn find_by_id(
    pool: &PgPool,
    product_id: &ProductId,
) -> Result<Option<InventoriesReadModel>, anyhow::Error> {
    sqlx::query_as!(
        InventoriesReadModel,
        r#"SELECT product_id as "product_id: _", inventory
           from inventories
           where product_id = $1;"#,
        &product_id as &ProductId
    )
    .fetch_optional(pool)
    .await
    .with_context(|| format!("Problem in find_by_product_id({product_id})"))
}

//------------------------- Projection --------------------------

pub(crate) struct InventoriesReadModelProjection {
    query: StreamQuery<i64, InventoryStream>,
    pool: PgPool,
}

impl InventoriesReadModelProjection {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            query: query!(InventoryStream),
        }
    }
}

#[async_trait]
impl EventListener<i64, InventoryStream> for InventoriesReadModelProjection {
    type Error = anyhow::Error;

    fn id(&self) -> &'static str {
        "inventories"
    }

    fn query(&self) -> &StreamQuery<i64, InventoryStream> {
        &self.query
    }

    async fn handle(&self, event: PersistedEvent<i64, InventoryStream>) -> Result<(), Self::Error> {
        let last_event_id = event.id();
        match event.into_inner() {
            InventoryStream::InventoryChanged {
                product_id,
                inventory,
            } => {
                sqlx::query!(
                    r#"INSERT INTO inventories (product_id, inventory, last_event_id)
                     VALUES ($1, $2, $3)
                     ON CONFLICT(product_id)
                     DO UPDATE SET
                       inventory = $2,
                       last_event_id = $3
                       WHERE inventories.last_event_id < $3;"#,
                    product_id as ProductId,
                    inventory,
                    last_event_id
                )
                .execute(&self.pool)
                .await
                .inspect_err(|e| error!("InventoriesReadModelProjection: Failed to handle InventoryChanged event {last_event_id} due to {e}"))?;
            }
        }

        Ok(())
    }
}

//-------------------------- Tests -------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{cart::ChangeInventoryCommand, create_eventstore_and_decider};
    use fake::Fake;

    #[sqlx::test]
    async fn it_finds_the_inventory_we_have_updated(pool: PgPool) {
        let (_event_store, decider) = create_eventstore_and_decider(&pool)
            .await
            .expect("EventStore and Decider should be created.");

        let projection = InventoriesReadModelProjection::new(pool.clone());

        let product_id = ProductId::new();
        let fake_inventory = || (0..1000).fake();
        let expected_inventory = fake_inventory();

        let commands = [
            ChangeInventoryCommand {
                product_id,
                inventory: fake_inventory(),
            },
            ChangeInventoryCommand {
                product_id: ProductId::new(),
                inventory: fake_inventory(),
            },
            ChangeInventoryCommand {
                product_id,
                inventory: expected_inventory,
            },
        ];
        for command in commands {
            let events = decider
                .make(command)
                .await
                .expect("Command should be successful.");
            for event in events.into_iter().map(|pe| {
                let id = pe.id();
                let inventory_event = InventoryStream::try_from(pe.into_inner()).unwrap();
                PersistedEvent::new(id, inventory_event)
            }) {
                projection
                    .handle(event)
                    .await
                    .expect("Event should be handled.");
            }
        }

        let expected_inventory = InventoriesReadModel {
            product_id,
            inventory: expected_inventory,
        };
        let found_inventory = find_by_id(&pool, &product_id).await.unwrap().unwrap();

        pool.close().await;

        assert_eq!(expected_inventory, found_inventory);
    }
}
