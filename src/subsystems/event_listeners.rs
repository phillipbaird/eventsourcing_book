use std::time::Duration;

use async_trait::async_trait;
use disintegrate_postgres::{PgEventListener, PgEventListenerConfig};
use futures::FutureExt;
use tokio::try_join;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::info;

use crate::{
    AppState,
    domain::cart::{
        CartItemsReadModelProjection, CartSubmittedEventHandler,
        CartsWithProductsReadModelProjection,
        inventories::InventoriesReadModelProjection,
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
        try_join!(
            event_listeners
                .start()
                .map(|result| result.map_err(anyhow::Error::new)),
            subsys.on_shutdown_requested().map(Ok::<(), anyhow::Error>)
        )?;

        info!("Event Listeners shutdown.");
        Ok(())
    }
}
