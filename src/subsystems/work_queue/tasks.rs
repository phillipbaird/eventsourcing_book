use anyhow::bail;
use strum_macros::Display;

use crate::{
    AppState,
    domain::{
        DomainEvent,
        cart::{PublishCartProcessorArgs, publish_cart_processor},
    },
};

use super::queue::Task;

#[derive(Debug, Clone, Display, serde::Serialize, serde::Deserialize)]
pub enum TaskDomainArgs {
    PublishCart(PublishCartProcessorArgs),
    TestingSuccess,
    TestingFailure,
}

impl TaskDomainArgs {
    /// Should return a DomainEvent if the work queue is responsible for storing a failure event.
    pub fn failure_event(&self) -> Option<DomainEvent> {
        match self {
            TaskDomainArgs::PublishCart(processor_args) => {
                Some(DomainEvent::CartPublicationFailed {
                    cart_id: processor_args.message.cart_id,
                })
            }
            TaskDomainArgs::TestingSuccess => None,
            TaskDomainArgs::TestingFailure => None,
        }
    }

    /// Should return a DomainEvent only if the Work Queue is responsible for storing a success
    /// event.
    pub fn success_event(&self) -> Option<DomainEvent> {
        match self {
            TaskDomainArgs::PublishCart(_) => None,
            TaskDomainArgs::TestingSuccess => None,
            TaskDomainArgs::TestingFailure => None,
        }
    }
}

pub async fn handle_task(state: &AppState, task: Task) -> Result<(), anyhow::Error> {
    match task.domain_args {
        TaskDomainArgs::PublishCart(args) => {
            publish_cart_processor(&state.settings.kafka, &state.event_store, args)
                .await
                .map_err(Into::<anyhow::Error>::into)
        }
        TaskDomainArgs::TestingSuccess => Ok(()),
        TaskDomainArgs::TestingFailure => bail!("Failed as expected."),
    }
}
