use cart_server::domain::{
    cart::{
        carts_with_products::find_by_product_id, AddItemCommand, CartId, ChangePriceCommand,
        ProductId,
    },
    create_eventstore_and_decider, Price,
};
use fake::{Fake, Faker};
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::select;

use crate::test_utils::{assert_until_eq, start_test_server};

#[sqlx::test]
#[serial]
async fn it_correctly_archives_items_from_carts(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (server_handle, server_pool, pool, _) = start_test_server(pool_options, connect_options).await;

    let (_event_store, decider) = create_eventstore_and_decider(&pool)
        .await
        .expect("Decider should be created.");

    let product_id = ProductId::new();

    // Create multiple carts with a number of items including a product of interest.
    let cart_count: usize = 5;
    for _ in 1..=cart_count {
        let cart_id = CartId::new();
        for item_index in 1..=3 {
            let this_product = if item_index != 1 {
                ProductId::new()
            } else {
                product_id
            };
            let decision = AddItemCommand {
                cart_id,
                product_id: this_product,
                ..Faker.fake()
            };
            decider
                .make(decision)
                .await
                .expect("Command should be successful.");
        }
    }

    // Wait until the read model reports the correct number of carts containing the product.
    assert_until_eq(
        || async {
            find_by_product_id(&pool, &product_id)
                .await
                .map(|v| v.len())
        },
        cart_count,
        format!("Waiting for Carts with Product to equal {cart_count}").as_str(),
    )
    .await;

    // Change the price of our product of interest.
    let decision = ChangePriceCommand {
        product_id,
        old_price: Price.fake(),
        new_price: Price.fake(),
    };
    decider
        .make(decision)
        .await
        .expect("Command should be successful.");

    // There should be zero carts containing the product once the PriceChanged event has been processed.
    select! {
        result = server_handle => {
            println!("Server terminated prematurely with result {result:?}.");
        }
        _ = assert_until_eq(
            || async { find_by_product_id(&pool, &product_id).await.map(|v| v.len()) },
            0,
            "Waiting for Carts with Product to equal 0"
        ) => {
            println!("Product has been archived.");
        }
    };

    server_pool.close().await;
    pool.close().await;
}
