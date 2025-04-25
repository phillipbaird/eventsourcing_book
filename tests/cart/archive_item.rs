use axum::http::StatusCode;
use cart_server::domain::{
    cart::{
        carts_with_products::CartsWithProductsReadModel, AddItemPayload, CartId,
        ChangePricePayload, ProductId,
    },
    fake::Price,
};
use fake::{Fake, Faker};
use httpc_test::Client;
use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::select;
use uuid::Uuid;

use crate::test_utils::{assert_until_eq, start_test_server};

async fn carts_with_products_len(
    client: &Client,
    product_uuid: &Uuid,
) -> httpc_test::Result<usize> {
    let res = client
        .do_get(&format!("/cartswithproducts/{product_uuid}"))
        .await
        .unwrap();
    res.json_body_as::<Vec<CartsWithProductsReadModel>>()
        .map(|vec| vec.len())
}

#[sqlx::test]
#[serial]
async fn it_correctly_archives_items_from_carts(
    _pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (server_handle, app_state) = start_test_server(connect_options.clone()).await;

    let url = format!("http://{}", app_state.settings.application.address());
    let client = httpc_test::new_client(url).expect("Expected client to be created.");

    let product_id: Uuid = ProductId::new().into();

    // Create multiple carts with a number of items including a product of interest.
    let cart_count: usize = 5;
    for _ in 1..=cart_count {
        let cart_id: Uuid = CartId::new().into();
        for item_index in 1..=3 {
            let this_product: Uuid = if item_index != 1 {
                ProductId::new().into()
            } else {
                product_id
            };
            let payload = AddItemPayload {
                cart_id,
                product_id: this_product,
                ..Faker.fake()
            };
            let json_payload =
                serde_json::to_value(&payload).expect("Expected payload to serialise.");
            let res = client
                .do_post(&format!("/additem/{cart_id}"), json_payload)
                .await
                .expect("AddItem command should be successful.");
            assert_eq!(res.status(), StatusCode::OK);
        }
    }

    // Wait until the read model reports the correct number of carts containing the product.
    assert_until_eq(
        || async { carts_with_products_len(&client, &product_id).await },
        cart_count,
        format!("Waiting for Carts with Product to equal {cart_count}").as_str(),
    )
    .await;

    // Change the price of our product of interest.
    let payload = ChangePricePayload {
        product_id,
        old_price: Price.fake(),
        new_price: Price.fake(),
    };
    let json_payload = serde_json::to_value(&payload).expect("Expected payload to serialise.");
    let res = client
        .do_post(&format!("/changeprice/{product_id}"), json_payload)
        .await
        .expect("ChangePrice cmmand should be successful.");
    assert_eq!(res.status(), StatusCode::OK);

    // There should be zero carts containing the product once the PriceChanged event has been processed.
    select! {
        result = server_handle => {
            println!("Server terminated prematurely with result {result:?}.");
        }
        _ = assert_until_eq(
            || async { carts_with_products_len(&client, &product_id).await },
            0,
            "Waiting for Carts with Product to equal 0"
        ) => {
            println!("Product has been archived.");
        }
    };

    app_state.pool.close().await;
}
