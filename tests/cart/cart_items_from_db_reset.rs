use cart_server::domain::{cart::{cart_items_from_db_read_model, cart_items_from_db_read_model_reset, AddItemCommand, CartId}, create_eventstore_and_decider};
use fake::{Faker, Fake};
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::test_utils::{assert_until_eq, start_test_server};

#[sqlx::test]
#[serial]
async fn it_resets_cart_items_read_model(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (server_handle, server_pool, pool, _) = start_test_server(pool_options.clone(), connect_options.clone()).await;

    let (_, decider) = create_eventstore_and_decider(&pool)
        .await
        .expect("Decider should be created.");


    let cart_id = CartId::new();

    let add_item_cmd = AddItemCommand {
            cart_id,
            ..Faker.fake()
    };
    decider.make(add_item_cmd).await.expect("Command should be successful.");

    // Wait until a read model is returned.
    assert_until_eq(
        || async { cart_items_from_db_read_model(&pool, &cart_id).await.map(|maybe| maybe.is_some()) },
        true,
        "Waiting for read model for cart.",
    )
    .await;
  
    // Shutdown the server.
    server_handle.abort();
    server_pool.close().await;

    // Reset the read model.
    cart_items_from_db_read_model_reset(&pool).await.expect("Read model should reset.");

    // Confirm there is no read model for cart.
    let result = cart_items_from_db_read_model(&pool, &cart_id).await.expect("Result should be ok.");
    assert!(result.is_none());
    pool.close().await;

    // Restart the server.
    let (server_handle, server_pool, pool, _) = start_test_server(pool_options, connect_options).await;

    // Confirm read model rebuilt.
    assert_until_eq(
        || async { cart_items_from_db_read_model(&pool, &cart_id).await.map(|maybe| maybe.is_some()) },
        true,
        "Waiting for read model for cart.",
    )
    .await;

    server_handle.abort();
    server_pool.close().await;
    pool.close().await;
}
