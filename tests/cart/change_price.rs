use cart_server::{
    domain::{
        PricingStream,
        cart::{PriceChangeTranslator, PriceChangedMessage, ProductId},
        create_eventstore_and_decider,
        fake::Price,
    },
    subsystems::KafkaMessageHandler,
};
use disintegrate::{EventStore, query};
use fake::Fake;
use futures::stream::StreamExt;
use rust_decimal::Decimal;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

use crate::test_utils::{assert_until_eq, create_producer, send_external_event, start_test_server};

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
    let (shutdown_token, settings) = start_test_server(connect_options.clone()).await;

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
    let producer = create_producer(&settings.kafka);
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

    // Creating a pool for the test to workaround this issue: https://github.com/launchbadge/sqlx/issues/2567
    let pool = pool_options
        .connect_with(connect_options)
        .await
        .expect("Expected pool to be created.");

    let (event_store, _decider) = create_eventstore_and_decider(&pool)
        .await
        .expect("EventStore should be created.");

    assert_until_eq(
        || async { find_price_changed_event(&event_store, &product_id).await },
        Some(expected_event),
        "Waiting for PriceChanged event from Kafka."
    ).await;
    println!("Price Change Event found!");

    shutdown_token.cancel();
}
