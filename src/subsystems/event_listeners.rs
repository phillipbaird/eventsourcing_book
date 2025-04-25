use std::time::Duration;

use async_trait::async_trait;
use disintegrate_postgres::{PgEventListener, PgEventListenerConfig};
use tokio::select;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{
    AppState,
    domain::cart::{
        CartItemsReadModelProjection, CartSubmittedEventHandler,
        CartsWithProductsReadModelProjection,
        InventoriesReadModelProjection,
    },
};

pub struct EventListeners {
    state: AppState,
}

impl EventListeners {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for EventListeners {
    async fn run(self, subsys: SubsystemHandle) -> Result<(), anyhow::Error> {
        let event_listeners = PgEventListener::builder(self.state.event_store)
            .register_listener(
                CartItemsReadModelProjection::new(self.state.pool.clone()),
                PgEventListenerConfig::poller(Duration::from_secs(5)).with_notifier(),
            )
            .register_listener(
                CartsWithProductsReadModelProjection::new(
                    self.state.pool.clone(),
                    self.state.decider,
                ),
                PgEventListenerConfig::poller(Duration::from_secs(5)).with_notifier(),
            )
            .register_listener(
                CartSubmittedEventHandler::new(self.state.work_queue.clone()),
                PgEventListenerConfig::poller(Duration::from_secs(5)).with_notifier(),
            )
            .register_listener(
                InventoriesReadModelProjection::new(self.state.pool),
                PgEventListenerConfig::poller(Duration::from_secs(5)).with_notifier(),
            );

        info!("Event Listeners starting.");
        let cancellation_token = CancellationToken::new();
        let cancellation_token_clone = cancellation_token.clone();
        let shutdown_handle = async move {
            cancellation_token_clone.cancelled().await;
        };
        select!( 
            result = event_listeners.start_with_shutdown(shutdown_handle) => {
                error!("Event listeners completed with {result:?}");
            }
            // .map(|result| result.map_err(anyhow::Error::new))
            _ = subsys.on_shutdown_requested() => {
                cancellation_token.cancel();
                info!("Event Listeners shutdown.");
            }
        );
        Ok(())
    }
}
