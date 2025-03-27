use serial_test::serial;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::test_utils::start_test_server;

#[sqlx::test]
#[serial]
async fn it_resets_cart_items_read_model(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let (server_handle, server_pool, pool, _) = start_test_server(pool_options, connect_options).await;


}
