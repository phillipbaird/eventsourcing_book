use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use disintegrate::EventStore;
use futures::{StreamExt, stream};
use tokio::select;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::{error, info};

use crate::AppState;

use super::tasks::handle_task;

const CONCURRENCY: usize = 10;

pub struct WorkQueueSubsystem {
    state: AppState,
}

impl WorkQueueSubsystem {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn start(&self) {
        loop {
            let tasks = match self.state.work_queue.pull(CONCURRENCY as i64).await {
                Ok(tasks) => tasks,
                Err(err) => {
                    error!("WorkQueueSubSystem: pulling tasks failed with {err}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    Vec::new()
                }
            };

            stream::iter(tasks)
                .for_each_concurrent(CONCURRENCY, |task| async {
                    let task_id = task.task_id;
                    let success_event = task.domain_args.success_event();
                    let failure_event = task.domain_args.failure_event();
                    let result = match handle_task(&self.state, task).await {
                        Ok(_) => {
                            match self.state.work_queue.delete_task(task_id).await {
                                Ok(_) => {
                                    if let Some(success_event) = success_event {
                                        self.state.event_store.append_without_validation(vec![success_event.clone()]).await
                                            .map(|_| ())
                                            .with_context(|| format!("Failed to append event {success_event:?}"))
                                    } else {
                                        Ok(())
                                    }
                                },
                                Err(_) => todo!(),
                            }
                        },
                        Err(err) => {
                            error!("WorkQueueSubSystem: handling task({task_id}) failed with {err}");
                            match self.state.work_queue.fail_task(task_id).await {
                                Ok(true) => {
                                    if let Some(failure_event) = failure_event {
                                        self.state.event_store.append_without_validation(vec![failure_event.clone()]).await
                                            .map(|_| ())
                                            .with_context(|| format!("Failed to append event {failure_event:?}"))
                                    } else {
                                        Ok(())
                                    }
                                },
                                Ok(false) => Ok(()),
                                Err(e) => Err(e),
                            }
                        }
                    };

                    match result {
                        Ok(_) => {}
                        Err(err) => {
                            error!("WorkQueueSubSystem: deleting task or failing task failed with {err}");
                        }
                    }
                })
                .await;

            // sleep not to overload the database
            tokio::time::sleep(Duration::from_millis(125)).await;
        }
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for WorkQueueSubsystem {
    async fn run(self, subsys: SubsystemHandle) -> Result<(), anyhow::Error> {
        info!("Work queue starting.");
        select!(
            _ = self.start() => {
                error!("Work queue stopped.");
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Work queue shutdown.");
            }
        );
        Ok(())
    }
}
