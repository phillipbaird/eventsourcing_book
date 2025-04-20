use cart_server::{
    domain::cart::{
        InventoryChangedMessage, InventoryChangedTranslator, ProductId,
        inventories::{InventoriesReadModel, find_by_id},
    },
    subsystems::KafkaMessageHandler,
};
use fake::Fake;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::select;
use uuid::Uuid;

use crate::test_utils::{assert_until_eq, create_producer, send_external_event, start_test_server};

/// This integration test sends an event to the inventory-changed kafka topic.
/// It then receives the message back and responds by processing ChangeInventoryCommand.
/// The command produces an internal InventoryChaged. The event is then used to update a read model.
#[sqlx::test]
#[serial]
async fn kafka_message_changes_inventory(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (server_handle, server_pool, pool, kafka_settings) =
        start_test_server(pool_options, connect_options).await;

    let product_id = ProductId::new();
    let product_uuid: Uuid = product_id.into();
    let inventory: i32 = (0..1000).fake();

    // Send a test event to Kafka.
    let external_message = InventoryChangedMessage {
        product_uuid,
        inventory,
    };
    let producer = create_producer(&kafka_settings);
    send_external_event(
        &producer,
        InventoryChangedTranslator::TOPIC,
        product_uuid.to_string(),
        external_message,
    )
    .await;

    let expected_inventory = Some(InventoriesReadModel {
        product_id,
        inventory,
    });

    select! {
        result = server_handle => {
            println!("Server terminated prematurely with result {result:?}.");
        }
        _ = assert_until_eq(
                || async { find_by_id(&pool, &product_id).await },
                expected_inventory,
                "Waiting for inventory to be updated."
        ) => {
            println!("Found the inventory had changed.");
        }
    };

    server_pool.close().await;
    pool.close().await;
}
