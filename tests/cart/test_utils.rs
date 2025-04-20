use std::{future::Future, time::Duration};

use cart_server::{
    construct_app_state,
    infra::{KafkaSettings, get_config_settings},
    start_server,
};

use rdkafka::{
    ClientConfig,
    producer::{FutureProducer, FutureRecord},
    util::Timeout,
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::task::JoinHandle;

/// Asserts that a function returns an expected value or retries until it does.
/// Retries every 500ms if the values do not match.
/// Will fail immediately on an error or after 60 retries (30 seconds).
pub async fn assert_until_eq<F, Fut, T, E>(f: F, expected_value: T, label: &str)
where
    F: Fn() -> Fut,
    E: std::fmt::Debug,
    Fut: Future<Output = Result<T, E>>,
    T: PartialEq + std::fmt::Debug,
{
    let delay_ms = 500;
    let max_times = 60;
    let mut times: usize = 0;
    let mut result: T = f().await.unwrap();
    while times < max_times {
        times += 1;
        if result == expected_value {
            break;
        } else {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            println!("Retry #{times} {label}");
            result = f().await.unwrap();
        }
    }
    assert_eq!(result, expected_value);
}

/// Constructs a Kafka producer for sending messages/events.
pub fn create_producer(settings: &KafkaSettings) -> FutureProducer {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", settings.bootstrap_servers.clone())
        .set(
            "message.timeout.ms",
            settings.session_timeout_ms.to_string(),
        )
        .create()
        .expect("Producer creation error");
    producer
}

/// Sends an event/message to Kafka.
pub async fn send_external_event<EE>(
    producer: &FutureProducer,
    topic: &str,
    key: String,
    external_event: EE,
) where
    EE: serde::Serialize,
{
    let payload = serde_json::to_vec(&external_event).expect("expected message to serialize");
    producer
        .send(
            FutureRecord::to(topic)
                .payload(&payload)
                .key(&key.to_string()),
            Timeout::Never,
        )
        .await
        .expect("Expected send to work.");
}

pub async fn start_test_server(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) -> (
    JoinHandle<Result<(), anyhow::Error>>,
    PgPool,
    PgPool,
    KafkaSettings,
) {
    let mut settings = get_config_settings().expect("Could not read application configuration.");
    settings.database.database_name = connect_options
        .get_database()
        .expect("Expected database name.")
        .into();
    let kafka_settings = settings.kafka.clone();
    let app_state = construct_app_state(settings)
        .await
        .expect("Expected AppState to be created.");
    let server_pool = app_state.pool.clone();
    let server_handle = tokio::task::spawn(start_server(app_state));
    // Creating a pool for the test to workaround this issue: https://github.com/launchbadge/sqlx/issues/2567
    let test_pool = pool_options
        .connect_with(connect_options)
        .await
        .expect("Expected pool to be created.");

    (server_handle, server_pool, test_pool, kafka_settings)
}
