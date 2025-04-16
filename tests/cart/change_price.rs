use cart_server::{
    domain::{
        cart::{PriceChangeTranslator, PriceChangedMessage, ProductId}, create_eventstore_and_decider, fake::Price, PricingStream
    }, subsystems::KafkaMessageHandler
};
use disintegrate::{query, EventStore};
use fake::Fake;
use futures::stream::StreamExt;
use rust_decimal::Decimal;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::select;
use uuid::Uuid;

use crate::test_utils::{
    assert_until_eq, create_producer, send_external_event, start_test_server,
};

/// Find PriceChanged event for a ProductID.
async fn find_price_changed_event(
    event_store: &cart_server::domain::EventStore,
    product_id: &ProductId,
) -> Result<Option<PricingStream>, disintegrate_postgres::Error> {
    let query = query!(PricingStream; product_id == product_id);
    let maybe_event = event_store.stream(&query).next().await;
    maybe_event
        .map(|result| result.map(|persisted_event| persisted_event.into_inner()))
        .transpose()
}

/// This integration test sends an event to the price-changes kafka topic.
/// It then receives the message back and responds by processing a ChangePriceCommand.
/// The command produces an internal PriceChanged event. We then check to ensure the event is
/// created.
#[sqlx::test]
#[serial]
async fn kafka_message_changes_price(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (server_handle, server_pool, pool, kafka_settings) = start_test_server(pool_options, connect_options).await;

    let product_id = ProductId::new();
    let product_uuid: Uuid = product_id.into();
    let old_price: Decimal = Price.fake();
    let new_price: Decimal = Price.fake();

    // Send a test event to Kafka.
    let external_event = PriceChangedMessage {
        product_uuid,
        old_price,
        new_price,
    };
    let producer = create_producer(&kafka_settings);
    send_external_event(
        &producer,
        PriceChangeTranslator::TOPIC,
        product_uuid.to_string(),
        external_event,
    )
    .await;

    let expected_event = PricingStream::PriceChanged {
        product_id,
        old_price,
        new_price,
    };

    let (event_store, _decider) = create_eventstore_and_decider(&pool).await.expect("EventStore should be created.");
    select! {
        result = server_handle => {
            println!("Server terminated prematurely with result {result:?}.");
        },
        _ = assert_until_eq(
            || async { find_price_changed_event(&event_store, &product_id).await }, 
            Some(expected_event), 
            "Waiting for PriceChanged event from Kafka."
        ) => {
            println!("Price Change Event found!");
        }
    };
  
    server_pool.close().await;
    pool.close().await;
}
