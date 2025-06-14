use async_trait::async_trait;
use axum::{
    Json,
    extract::State,
    routing::{get, post},
};
use futures::FutureExt;
use tokio::select;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

use crate::{AppState, domain::cart::carts_with_products_endpoint, infra::ClientError};

pub struct WebServer {
    state: AppState,
}

impl WebServer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for WebServer {
    async fn run(self, subsys: SubsystemHandle) -> Result<(), anyhow::Error> {
        let address = self.state.settings.application.address();

        let router = axum::Router::new()
            .route(
                "/additem/{cart_id}",
                post(crate::domain::cart::add_item_endpoint),
            )
            .route(
                "/{cart_id}/cartitems",
                get(crate::domain::cart::cart_items_endpoint),
            )
            .route(
                "/{cart_id}/cartitemsfromdb",
                get(crate::domain::cart::cart_items_from_db_endpoint),
            )
            .route(
                "/cartswithproducts/{product_id}",
                get(carts_with_products_endpoint),
            )
            .route(
                "/changeprice/{product_id}",
                post(crate::domain::cart::change_price_endpoint),
            )
            .route(
                "/clearcart/{cart_id}",
                post(crate::domain::cart::clear_cart_endpoint),
            )
            .route(
                "/inventories/{product_id}",
                get(crate::domain::cart::inventories_endpoint),
            )
            .route(
                "/removeitem/{cart_id}",
                post(crate::domain::cart::remove_item_endpoint),
            )
            .route(
                "/submitcart/{cart_id}",
                post(crate::domain::cart::submit_cart_endpoint),
            )
            .route("/healthcheck", get(health_check_endpoint))
            .layer(TraceLayer::new_for_http())
            .with_state(self.state);

        let listener = tokio::net::TcpListener::bind(address.clone())
            .await
            .inspect_err(|e| error!("Could not bind socket address {address}. Failed with {e}"))?;

        info!("Web server starting on http://{address}");
        select!(
            result = axum::serve(listener, router.into_make_service()).into_future().map(|result| result.map_err(anyhow::Error::new)) => {
                error!("Web server completed with {result:?}");
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Web server shutdown");
            }
        );
        Ok(())
    }
}

pub async fn health_check_endpoint(
    State(_app_state): State<AppState>,
) -> Result<Json<String>, ClientError> {
    Ok(Json("Ok".to_owned()))
}
