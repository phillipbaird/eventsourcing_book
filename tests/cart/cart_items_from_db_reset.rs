use axum::http::StatusCode;
use cart_server::domain::cart::{
    cart_items_from_db_read_model, cart_items_from_db_read_model_reset, AddItemPayload, CartId,
    CartItemsReadModel,
};
use fake::{Fake, Faker};
use httpc_test::Client;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

use crate::test_utils::{assert_until_eq, start_test_server};

async fn get_cart_items(
    client: &Client,
    cart_uuid: &Uuid,
) -> httpc_test::Result<Option<CartItemsReadModel>> {
    let res = client
        .do_get(&format!("/{cart_uuid}/cartitemsfromdb"))
        .await
        .unwrap();
    if res.status() == StatusCode::OK {
        res.json_body_as::<CartItemsReadModel>().map(Some)
    }
    else {
        Ok(None)
    }
}

#[sqlx::test]
#[serial]
async fn it_resets_cart_items_read_model(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (shutdown_token, settings) = start_test_server(connect_options.clone()).await;

    let url = format!("http://{}", settings.application.address());
    let client = httpc_test::new_client(url.clone()).expect("Expected client to be created.");

    let cart_id = CartId::new();
    let cart_uuid = cart_id.into();

    let payload = AddItemPayload {
        cart_id: cart_uuid,
        ..Faker.fake()
    };
    let json_payload = serde_json::to_value(&payload).expect("Expected payload to serialise.");
    let res = client
        .do_post(&format!("/additem/{cart_id}"), json_payload)
        .await
        .expect("AddItem command should be successful.");
    assert_eq!(res.status(), StatusCode::OK);

    // Wait until a read model is returned.
    assert_until_eq(
        || async {
            get_cart_items(&client, &cart_uuid)
                .await
                .map(|maybe| maybe.is_some())
        },
        true,
        "Waiting for read model for cart.",
    )
    .await;

    // Shutdown the server.
    shutdown_token.cancel();

    // Creating a pool for the test to workaround this issue: https://github.com/launchbadge/sqlx/issues/2567
    let pool = pool_options
        .connect_with(connect_options.clone())
        .await
        .expect("Expected pool to be created.");

    // Reset the read model.
    cart_items_from_db_read_model_reset(&pool)
        .await
        .expect("Read model should reset.");

    // Confirm there is no read model for cart.
    // We cannot check this via the web api because restarting the server will rebuild the read
    // model.
    let result = cart_items_from_db_read_model(&pool, &cart_id)
        .await
        .expect("Result should be ok.");
    assert!(result.is_none());
    pool.close().await;

    // Restart the server.
    let (shutdown_token, _) = start_test_server(connect_options).await;

    // Confirm read model rebuilt.
    let client = httpc_test::new_client(url).expect("Expected client to be created.");
    assert_until_eq(
        || async {
            get_cart_items(&client, &cart_uuid)
                .await
                .map(|maybe| maybe.is_some())
        },
        true,
        "Waiting for read model for cart.",
    )
    .await;

    // Shutdown the server.
    shutdown_token.cancel();
}
