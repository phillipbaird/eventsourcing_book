use axum::http::StatusCode;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::test_utils::start_test_server;

#[sqlx::test]
#[serial]
async fn the_webserver_responds_to_a_simple_get_request(
    _pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (shutdown_token, settings) = start_test_server(connect_options.clone()).await;

    let url = format!("http://{}", settings.application.address());
    let client = httpc_test::new_client(url).expect("Expected client to be created.");
    let res = client
        .do_get("/healthcheck")
        .await
        .expect("Health check should succeed.");

    shutdown_token.cancel();

    assert_eq!(res.status(), StatusCode::OK);
}
