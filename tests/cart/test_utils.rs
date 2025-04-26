use std::{future::Future, time::Duration};

use cart_server::{
    build_subsystems, construct_app_state,
    infra::{KafkaSettings, Settings, get_config_settings},
    test_server,
};

use rdkafka::{
    ClientConfig,
    producer::{FutureProducer, FutureRecord},
    util::Timeout,
};
use sqlx::postgres::PgConnectOptions;
use tokio_util::sync::CancellationToken;

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
    let mut result: T = f().await.inspect(|e| println!("{e:?}")).unwrap();
    while times < max_times {
        times += 1;
        if result == expected_value {
            break;
        } else {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            println!("Retry #{times} {label}");
            result = f().await.inspect(|e| println!("{e:?}")).unwrap();
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

pub async fn start_test_server(connect_options: PgConnectOptions) -> (CancellationToken, Settings) {
    let mut settings = get_config_settings().expect("Could not read application configuration.");
    settings.database.database_name = connect_options
        .get_database()
        .expect("Expected database name.")
        .into();
    let app_state = construct_app_state(settings.clone())
        .await
        .expect("Expected AppState to be created.");
    let pool = app_state.pool.clone();
    let subsystems = build_subsystems(app_state);
    let shutdown_token = subsystems._get_shutdown_token().clone();
    tokio::task::spawn(test_server(subsystems, pool));

    (shutdown_token, settings)
}
