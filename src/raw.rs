//! CloudFormation client extensions using raw `rusoto_cloudformation` types.
//!
//! This is a thin layer over the types in `rusoto_cloudformation` that provides convenient APIs to
//! perform long-running CloudFormation operations and await their results or observe their
//! progress.
//!
//! See the [`CloudFormationExt`] trait for more information.

use std::{future::Future, pin::Pin, time::Duration};

use async_stream::try_stream;
use chrono::{DateTime, Utc};
use rusoto_cloudformation::{
    CloudFormation, CreateChangeSetError, CreateChangeSetInput, DescribeChangeSetError,
    DescribeChangeSetInput, DescribeChangeSetOutput, DescribeStackEventsError,
    DescribeStackEventsInput, ExecuteChangeSetError, ExecuteChangeSetInput, StackEvent,
};
use rusoto_core::RusotoError;
use tokio::time::Instant;
use tokio_stream::Stream;

/// In-progress statuses for stack creation.
const CREATE_STACK_PROGRESS_STATUSES: &[&str] = &[
    "CREATE_IN_PROGRESS",
    "CREATE_FAILED",
    "ROLLBACK_IN_PROGRESS",
];

/// Terminal statuses for stack creation.
const CREATE_STACK_TERMINAL_STATUSES: &[&str] =
    &["CREATE_COMPLETE", "ROLLBACK_FAILED", "ROLLBACK_COMPLETE"];

/// In-progress statuses for stack update.
const UPDATE_STACK_PROGRESS_STATUSES: &[&str] = &[
    "UPDATE_IN_PROGRESS",
    "UPDATE_COMPLETE_CLEANUP_IN_PROGRESS",
    "UPDATE_ROLLBACK_IN_PROGRESS",
    "UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS",
];

/// Terminal statuses for stack update.
const UPDATE_STACK_TERMINAL_STATUSES: &[&str] = &[
    "UPDATE_COMPLETE",
    "UPDATE_ROLLBACK_FAILED",
    "UPDATE_ROLLBACK_COMPLETE",
];

/// In-progress statuses for change set creation.
const CREATE_CHANGE_SET_PROGRESS_STATUSES: &[&str] = &["CREATE_PENDING", "CREATE_IN_PROGRESS"];

/// Terminal statuses for change set creation.
const CREATE_CHANGE_SET_TERMINAL_STATUSES: &[&str] = &["CREATE_COMPLETE", "FAILED"];

/// Convenience alias for a `Box::pin`ned `Future`.
type PinBoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Convenience alias for a `Box::pin`ned `Stream` of `StackEvent`s.
pub type StackEventStream<'a> =
    Pin<Box<dyn Stream<Item = Result<StackEvent, RusotoError<DescribeStackEventsError>>> + 'a>>;

/// [`rusoto_cloudformation::CloudFormation`] extension trait that works directly with
/// `rusoto_cloudformation` types.
pub trait CloudFormationExt {
    /// Create a change set and wait for it to become available.
    ///
    /// This will call the `CreateChangeSet` API, but that only begins the creation process. If
    /// `CreateChangeSet` returns successfully, the `DescribeChangeSet` API is polled until the
    /// change set has settled.
    ///
    /// # Errors
    ///
    /// Any errors returned when calling the `CreateChangeSet` or `DescribeChangeSet` APIs are
    /// returned (via [`CreateChangeSetWaitError::CreateChangeSet`] and
    /// [`CreateChangeSetWaitError::DescribeChangeSet`] respectively).
    ///
    /// # Panics
    ///
    /// This will panic if the change set enters a status that is unexpected for creation. This
    /// would be a bug in CloudFormation itself or (more likely) a misunderstanding of its semantics
    /// that would require this library to be updated!
    fn create_change_set_wait(
        &self,
        input: CreateChangeSetInput,
    ) -> PinBoxFut<'_, Result<DescribeChangeSetOutput, CreateChangeSetWaitError>>;

    /// Execute a change set and return a stream of relevant stack events.
    ///
    /// This will call the `DescribeChangeSet` API to get the stack ID, followed by the
    /// `ExecuteChangeSet` API to commence the execution. If that returns successfully the
    /// `DescribeStackEvents` API is polled and the events are emitted through the returned
    /// `Stream`. The stream ends when the stack reaches a settled state.
    ///
    /// # Errors
    ///
    /// The returned `Future` will resolve to an `Err` if the `DescribeChangeSet` or
    /// `ExecuteChangeSet` API fails. Since any attempt to poll the `DescribeStackEvents` API might
    /// fail, each event is wrapped in a `Result` and so must be checked for errors.
    ///
    /// # Panics
    ///
    /// This will panic if the stack enters a status that is unexpected for updating. This would be
    /// a bug in CloudFormation itself or (more likely) a misunderstanding of its semantics that
    /// would require this library to be updated!
    fn execute_change_set_stream(
        &self,
        input: ExecuteChangeSetInput,
    ) -> PinBoxFut<Result<StackEventStream, ExecuteChangeSetStreamError>>;
}

impl<T> CloudFormationExt for T
where
    T: CloudFormation,
{
    fn create_change_set_wait(
        &self,
        input: CreateChangeSetInput,
    ) -> PinBoxFut<'_, Result<DescribeChangeSetOutput, CreateChangeSetWaitError>> {
        Box::pin(create_change_set_wait(self, input))
    }

    fn execute_change_set_stream(
        &self,
        input: ExecuteChangeSetInput,
    ) -> PinBoxFut<Result<StackEventStream, ExecuteChangeSetStreamError>> {
        Box::pin(execute_change_set_stream(self, input))
    }
}

/// Errors that can occur during [`create_change_set_wait`].
///
/// [`create_change_set_wait`]: CloudFormationExt::create_change_set_wait
#[derive(Debug, thiserror::Error)]
pub enum CreateChangeSetWaitError {
    /// The `CreateChangeSet` operation returned an error.
    #[error("CreateChangeSet error: {0}")]
    CreateChangeSet(#[from] RusotoError<CreateChangeSetError>),

    /// A `DescribeChangeSet` operation returned an error.
    #[error("DescribeChangeSet error: {0}")]
    DescribeChangeSet(#[from] RusotoError<DescribeChangeSetError>),
}

async fn create_change_set_wait<Client: CloudFormation>(
    client: &Client,
    input: CreateChangeSetInput,
) -> Result<DescribeChangeSetOutput, CreateChangeSetWaitError> {
    let change_set_id = client
        .create_change_set(input)
        .await?
        .id
        .expect("CreateChangeSetOutput without id");

    let describe_change_set_input = DescribeChangeSetInput {
        change_set_name: change_set_id.clone(),
        ..DescribeChangeSetInput::default()
    };
    let mut interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(1),
        Duration::from_secs(1),
    );
    loop {
        interval.tick().await;

        let change_set = client
            .describe_change_set(describe_change_set_input.clone())
            .await?;
        let change_set_status = change_set
            .status
            .as_deref()
            .expect("DescribeChangeSet without status");
        if CREATE_CHANGE_SET_PROGRESS_STATUSES.contains(&change_set_status) {
            continue;
        }
        if CREATE_CHANGE_SET_TERMINAL_STATUSES.contains(&change_set_status) {
            return Ok(change_set);
        }
        panic!(
            "change set {} has inconsistent status for create: {}",
            change_set_id, change_set_status
        );
    }
}

/// Errors that can be returned by [`execute_change_set_stream`].
///
/// [`execute_change_set_stream`]: CloudFormationExt::execute_change_set_stream
#[derive(Debug, thiserror::Error)]
pub enum ExecuteChangeSetStreamError {
    /// The `DescribeChangeSet` operation returned an error.
    #[error("{0}")]
    DescribeChangeSet(#[from] RusotoError<DescribeChangeSetError>),

    /// The `ExecuteChangeSet` operation returned an error.
    #[error("{0}")]
    ExecuteChangeSet(#[from] RusotoError<ExecuteChangeSetError>),
}

async fn execute_change_set_stream<Client: CloudFormation>(
    client: &Client,
    input: ExecuteChangeSetInput,
) -> Result<StackEventStream<'_>, ExecuteChangeSetStreamError> {
    let stack_id = client
        .describe_change_set(DescribeChangeSetInput {
            stack_name: input.stack_name.clone(),
            change_set_name: input.change_set_name.clone(),
            ..DescribeChangeSetInput::default()
        })
        .await?
        .stack_id
        .expect("DescribeChangeSetOutput without stack_id");

    let mut event_cutoff = format_timestamp(Utc::now());
    client.execute_change_set(input).await?;

    let describe_stack_events_input = DescribeStackEventsInput {
        stack_name: Some(stack_id.clone()),
        ..DescribeStackEventsInput::default()
    };
    let mut interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let mut statuses = None;

    Ok(Box::pin(try_stream! {
        loop {
            interval.tick().await;

            let stack_events: Vec<_> = client
                .describe_stack_events(describe_stack_events_input.clone())
                .await?
                .stack_events
                .expect("DescribeStackEventsOutput without stack_events")
                .into_iter()
                .take_while(|event| event.timestamp > event_cutoff)
                .collect();

            if let Some(stack_event) = stack_events.first() {
                event_cutoff = stack_event.timestamp.clone();
            }

            if let (None, Some(stack_event)) = (statuses, stack_events.last()) {
                statuses = Some(match stack_event.resource_status.as_deref() {
                    Some("CREATE_IN_PROGRESS") => (
                        CREATE_STACK_PROGRESS_STATUSES,
                        CREATE_STACK_TERMINAL_STATUSES,
                    ),
                    Some("UPDATE_IN_PROGRESS") => (
                        UPDATE_STACK_PROGRESS_STATUSES,
                        UPDATE_STACK_TERMINAL_STATUSES,
                    ),
                    _ => panic!(
                        "can't handle resource_status: {:?}",
                        stack_event.resource_status
                    ),
                });
            }

            for stack_event in stack_events.into_iter().rev() {
                if stack_event.physical_resource_id.as_ref() != Some(&stack_id) {
                    yield stack_event;
                } else {
                    let stack_status = stack_event
                        .resource_status
                        .as_deref()
                        .expect("StackEvent without resource_status");
                    if statuses.unwrap().0.contains(&stack_status) {
                        yield stack_event;
                        continue;
                    }
                    if statuses.unwrap().1.contains(&stack_status) {
                        yield stack_event;
                        return;
                    }
                    panic!(
                        "stack {} has inconsistent status for update: {}",
                        stack_id, stack_status
                    );
                }
            }
        }
    }))
}

/// Format a timestamp to the same format as CloudFormation.
fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use rusoto_cloudformation::CloudFormationClient;
    use rusoto_core::Region;

    use super::CloudFormationExt;

    #[test]
    fn cloudformation_client_impl() {
        let client = CloudFormationClient::new(Region::EuWest2);
        let _: &dyn CloudFormationExt = &client;
    }
}
