use std::time::Duration;

use crate::uuid_id;
use anyhow::Context;
use jiff::{Zoned, tz::TimeZone};
use jiff_sqlx::{DateTime, Timestamp, ToSqlx};
use sqlx::{PgPool, postgres::PgDatabaseError, types::Json};
use tracing::warn;

use super::tasks::TaskDomainArgs;

uuid_id!(TaskId);

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct TaskRow {
    pub task_id: TaskId,
    pub task_type: String,
    pub triggering_event: Option<i64>,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
    pub scheduled_for: jiff_sqlx::DateTime,
    pub next_attempt_at: jiff_sqlx::DateTime,
    pub failed_attempts: i32,
    pub status: TaskStatus,
    pub domain_args: Json<TaskDomainArgs>,
}

#[derive(Clone)]
pub struct Task {
    pub task_id: TaskId,
    pub domain_args: TaskDomainArgs,
}

impl From<TaskRow> for Task {
    fn from(row: TaskRow) -> Self {
        Task {
            task_id: row.task_id,
            domain_args: row.domain_args.0,
        }
    }
}

// We use a INT postgres representation for performance reasons
#[derive(Debug, Clone, sqlx::Type, PartialEq)]
#[repr(i32)]
pub enum TaskStatus {
    Queued,
    Running,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TaskArgs {
    pub trigger: TaskTrigger,
    pub limits: TaskLimit,
    pub domain_args: TaskDomainArgs,
}

#[derive(Debug, Clone)]
pub enum TaskLimit {
    MaxAttempts(i32),
    TimeoutAfter(Duration),
}

#[derive(Debug, Clone)]
pub enum TaskTrigger {
    Event(i64),
    ScheduleNow,
    ScheduleFor(jiff::civil::DateTime),
}
#[derive(Debug, Clone)]
pub struct WorkQueue {
    pool: PgPool,
}

impl WorkQueue {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn clear(&self) -> Result<(), anyhow::Error> {
        sqlx::query!("DELETE FROM queue")
            .execute(&self.pool)
            .await
            .context("Problem clearing work queue.")?;
        Ok(())
    }

    pub async fn delete_task(&self, task_id: TaskId) -> Result<(), anyhow::Error> {
        sqlx::query!("DELETE FROM queue WHERE task_id = $1", task_id as TaskId)
            .execute(&self.pool)
            .await
            .with_context(|| format!("Problem deleting Task {task_id}."))?;

        Ok(())
    }

    pub async fn fail_task(&self, task_id: TaskId) -> Result<bool, anyhow::Error> {
        let now = jiff::Timestamp::now().to_sqlx();
        let permanent_failure = sqlx::query_scalar!(
            "UPDATE queue
             SET 
                 status = $1, 
                 updated_at = $2, 
                 failed_attempts = failed_attempts + 1,
                 next_attempt_at = next_attempt_at + (INTERVAL '125 millisecond' * pow(2, failed_attempts))
             WHERE task_id = $3
             RETURNING
                (failed_attempts >= max_attempts or next_attempt_at >= timeout_at)
            ",
            TaskStatus::Queued as TaskStatus,
            now as Timestamp,
            task_id as TaskId
        )
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("Problem marking Task {task_id} as failed."))?.unwrap_or_default();
        Ok(permanent_failure)
    }

    pub async fn fetch(&self, task_id: TaskId) -> Result<TaskRow, anyhow::Error> {
        sqlx::query_as!(
            TaskRow,
            r#"
            SELECT
                task_id as "task_id: _",
                task_type,
                triggering_event,
                created_at as "created_at: _",
                updated_at as "updated_at: _",
                scheduled_for as "scheduled_for: _",
                next_attempt_at as "next_attempt_at: _",
                failed_attempts,
                status as "status: _",
                domain_args as "domain_args:_"
            FROM queue WHERE task_id = $1"#,
            task_id as TaskId
        )
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("Problem retrieving Task {task_id}."))
    }

    pub async fn pull(&self, number_of_tasks: i64) -> Result<Vec<Task>, anyhow::Error> {
        let now_timestamp = jiff::Timestamp::now();
        let now_datetime: jiff::civil::DateTime = now_timestamp.to_zoned(TimeZone::system()).into();
        let now_timestatmp = now_timestamp.to_sqlx();
        let now_datetime = now_datetime.to_sqlx();

        let tasks: Vec<TaskRow> = sqlx::query_as!(
            TaskRow,
            r#"
            UPDATE queue
            SET status = $1, updated_at = $2
            WHERE task_id IN (
                SELECT task_id
                FROM queue
                WHERE 
                   status = $3 
                   AND scheduled_for <= $4
                   AND next_attempt_at <= $4
                   AND next_attempt_at <= timeout_at
                   AND failed_attempts < max_attempts
                ORDER BY scheduled_for
                FOR UPDATE SKIP LOCKED
                LIMIT $5
            )
            RETURNING 
                task_id as "task_id: _",
                task_type,
                triggering_event,
                created_at as "created_at: _",
                updated_at as "updated_at: _",
                scheduled_for as "scheduled_for: _",
                next_attempt_at as "next_attempt_at: _",
                failed_attempts,
                status as "status: _",
                domain_args as "domain_args:_"
            "#,
            TaskStatus::Running as TaskStatus,
            now_timestatmp as Timestamp,
            TaskStatus::Queued as TaskStatus,
            now_datetime as DateTime,
            number_of_tasks
        )
        .fetch_all(&self.pool)
        .await
        .context("Problem pulling tasks from work queue.")?;

        Ok(tasks.into_iter().map(Into::into).collect())
    }

    pub async fn push(&self, args: TaskArgs) -> Result<(), sqlx::Error> {
        let triggering_event_id: Option<i64> = if let TaskTrigger::Event(event_id) = args.trigger {
            Some(event_id)
        } else {
            None
        };

        let civil_scheduled_at = match args.trigger {
            TaskTrigger::Event(_) | TaskTrigger::ScheduleNow => Zoned::now().datetime(),
            TaskTrigger::ScheduleFor(date_time) => date_time,
        };
        let scheduled_for = civil_scheduled_at.to_sqlx();

        let civil_timeout_at = match args.limits {
            TaskLimit::MaxAttempts(_) => civil_scheduled_at + Duration::from_secs(3600 * 24),
            TaskLimit::TimeoutAfter(duration) => civil_scheduled_at + duration,
        };
        let timeout_at = civil_timeout_at.to_sqlx();

        let max_attempts: i32 = match args.limits {
            TaskLimit::MaxAttempts(max_attempts) => max_attempts,
            TaskLimit::TimeoutAfter(_) => 1_000_000,
        };
        let task_type = args.domain_args.to_string();
        let domain_args = Json(args.domain_args);
        let now = jiff::Timestamp::now().to_sqlx();
        let task_id = TaskId::new();

        match sqlx::query!(
            "INSERT INTO queue
            (task_id, task_type, triggering_event, created_at, updated_at, scheduled_for, next_attempt_at, timeout_at, 
             failed_attempts, max_attempts, status, domain_args)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            task_id as TaskId,
            task_type,
            triggering_event_id,
            now as Timestamp,
            now as Timestamp,
            scheduled_for as DateTime,
            scheduled_for as DateTime,
            timeout_at as DateTime,
            0i32,
            max_attempts,
            TaskStatus::Queued as TaskStatus,
            domain_args as Json<TaskDomainArgs>,
        )
        .execute(&self.pool)
        .await {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db_err)) => {
                let pg_error = db_err.downcast_ref::<PgDatabaseError>();
                if pg_error.constraint() == Some("queue_task_type_triggering_event_key") {
                    warn!("Ignoring duplicate request for task {task_type} triggered by event {triggering_event_id:?}");
                    Ok(())
                } else {
                    Err(sqlx::Error::Database(db_err))
                }
            }
            Err(e) => Err(e),
        }
    }
}

//---------------------------- Tests -----------------------------

#[cfg(test)]
mod tests {
    use crate::domain::cart::{
        CartId, ExternalPublishCart, OrderedProduct, ProductId, PublishCartProcessorArgs,
    };

    use super::*;
    use fake::{Fake, Faker};

    #[sqlx::test]
    async fn tasks_triggered_from_an_event_should_be_idempotent(pool: PgPool) {
        let triggering_event_id: i64 = Faker.fake();

        let task_args = TaskArgs {
            trigger: TaskTrigger::Event(triggering_event_id),
            limits: TaskLimit::TimeoutAfter(Duration::from_secs(3600)),
            domain_args: TaskDomainArgs::PublishCart(PublishCartProcessorArgs {
                triggering_event_id,
                message: ExternalPublishCart {
                    cart_id: CartId::new(),
                    ordered_product: vec![OrderedProduct {
                        product_id: ProductId::new(),
                        price: Faker.fake(),
                    }],
                    total_price: Faker.fake(),
                },
            }),
        };

        let work_queue = WorkQueue::new(pool);

        work_queue
            .push(task_args.clone())
            .await
            .expect("Task should be queued.");
        work_queue
            .push(task_args)
            .await
            .expect("Duplicate of task should be ignored.");

        let tasks = work_queue
            .pull(1000)
            .await
            .expect("Tasks should be returned.");
        assert_eq!(
            tasks.len(),
            1,
            "There should be only one task in the queue."
        );
    }

    #[sqlx::test]
    async fn scheduled_tasks_should_not_be_seen_as_duplicates(pool: PgPool) {
        let triggering_event_id: i64 = Faker.fake();

        let task_args = TaskArgs {
            trigger: TaskTrigger::ScheduleNow,
            limits: TaskLimit::TimeoutAfter(Duration::from_secs(3600)),
            domain_args: TaskDomainArgs::PublishCart(PublishCartProcessorArgs {
                triggering_event_id,
                message: ExternalPublishCart {
                    cart_id: CartId::new(),
                    ordered_product: vec![OrderedProduct {
                        product_id: ProductId::new(),
                        price: Faker.fake(),
                    }],
                    total_price: Faker.fake(),
                },
            }),
        };

        let work_queue = WorkQueue::new(pool);

        work_queue
            .push(task_args.clone())
            .await
            .expect("Task should be queued.");
        work_queue
            .push(task_args)
            .await
            .expect("Task should be queued.");

        let tasks = work_queue
            .pull(1000)
            .await
            .expect("Tasks should be returned.");
        assert_eq!(tasks.len(), 2, "There should be two tasks in the queue.");
    }

    #[sqlx::test]
    async fn failing_task_should_calculate_next_attempt_correctly(pool: PgPool) {
        let work_queue = WorkQueue::new(pool.clone());

        let task_args = TaskArgs {
            trigger: TaskTrigger::ScheduleNow,
            limits: TaskLimit::MaxAttempts(2),
            domain_args: TaskDomainArgs::TestingFailure,
        };

        work_queue
            .push(task_args)
            .await
            .expect("Task should be queued.");

        let tasks = work_queue.pull(100).await.expect("Task should be found.");
        assert_eq!(tasks.len(), 1, "There should be a single task.");
        let task = tasks.first().expect("Task should be there.");
        let task_id = task.task_id;
        let task_row = work_queue
            .fetch(task_id)
            .await
            .expect("TaskRow should be round.");
        let attempt_0_at = task_row.next_attempt_at.to_jiff();

        let is_last_failure = work_queue
            .fail_task(task_id)
            .await
            .expect("Task should be marked as failed.");
        assert!(
            !is_last_failure,
            "MaxAttempts = 2 so first failure should not be its last."
        );
        let task_row = work_queue
            .fetch(task_id)
            .await
            .expect("TaskRow should be found.");
        assert_eq!(task_row.failed_attempts, 1);
        let attempt_1_at = task_row.next_attempt_at.to_jiff();
        let difference = (attempt_1_at - attempt_0_at).get_milliseconds();
        assert_eq!(difference, 125);

        let is_last_failure = work_queue
            .fail_task(task_id)
            .await
            .expect("Task should be marked as failed.");
        assert!(
            is_last_failure,
            "MaxAttempts = 2 so second failure should be its last."
        );
        let task_row = work_queue
            .fetch(task_id)
            .await
            .expect("TaskRow should be found.");
        assert_eq!(task_row.failed_attempts, 2);
        let attempt_2_at = task_row.next_attempt_at.to_jiff();
        let difference = (attempt_2_at - attempt_1_at).get_milliseconds();
        assert_eq!(difference, 250);
    }
}
