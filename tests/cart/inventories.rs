use cart_server::{
    domain::cart::{
        InventoriesReadModel, InventoryChangedMessage, InventoryChangedTranslator, ProductId
    },
    subsystems::KafkaMessageHandler,
};
use fake::Fake;
use httpc_test::Client;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

use crate::test_utils::{assert_until_eq, create_producer, send_external_event, start_test_server};

async fn get_inventories(
    client: &Client,
    product_uuid: &Uuid,
) -> httpc_test::Result<Option<InventoriesReadModel>> {
    let res = client
        .do_get(&format!("/inventories/{product_uuid}"))
        .await
        .unwrap();
    res.json_body_as::<Option<InventoriesReadModel>>()
}


/// This integration test sends an event to the inventory-changed kafka topic.
/// It then receives the message back and responds by processing ChangeInventoryCommand.
/// The command produces an internal InventoryChaged. The event is then used to update a read model.
#[sqlx::test]
#[serial]
async fn kafka_message_changes_inventory(
    _: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (shutdown_token, settings) =
        start_test_server(connect_options.clone()).await;

    let product_id = ProductId::new();
    let product_uuid: Uuid = product_id.into();
    let inventory: i32 = (0..1000).fake();

    // Send a test event to Kafka.
    let external_message = InventoryChangedMessage {
        product_uuid,
        inventory,
    };
    let producer = create_producer(&settings.kafka);
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

    let url = format!("http://{}", settings.application.address());
    let client = httpc_test::new_client(url).expect("Expected client to be created.");

    assert_until_eq(
        || async { get_inventories(&client, &product_uuid).await },
        expected_inventory,
        "Waiting for inventory to be updated."
    ).await;
    println!("Found the inventory had changed.");

    shutdown_token.cancel();
}
