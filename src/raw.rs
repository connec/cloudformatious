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
    CloudFormation, CreateChangeSetError, CreateChangeSetInput, CreateStackError, CreateStackInput,
    DeleteStackError, DeleteStackInput, DescribeChangeSetError, DescribeChangeSetInput,
    DescribeChangeSetOutput, DescribeStackEventsError, DescribeStackEventsInput,
    DescribeStacksError, DescribeStacksInput, Stack, StackEvent, UpdateStackError,
    UpdateStackInput,
};
use rusoto_core::RusotoError;
use tokio::time::Instant;
use tokio_stream::Stream;

/// [`rusoto_cloudformation::CloudFormation`] extension trait that works directly with
/// `rusoto_cloudformation` types.
pub trait CloudFormationExt {
    /// Create a stack and wait for it to complete.
    ///
    /// This will call the `CreateStack` API, but this only begins the creation process. If
    /// `CreateStack` returns successfully, the `DescribeStacks` API is polled until the stack has
    /// settled.
    ///
    /// # Errors
    ///
    /// Any errors returned when calling the `CreateStack` or `DescribeStacks` APIs are returned
    /// (via [`CreateStackCheckedError::CreateStack`] and
    /// [`CreateStackCheckedError::DescribeStacks`] respectively).
    ///
    /// If the stack settled with `ROLLBACK_COMPLETE` or `ROLLBACK_FAILED` status,
    /// [`CreateStackCheckedError::Failed`] is returned.
    ///
    /// If the stack was seen with an unexpected status, [`CreateStackCheckedError::Conflict`] is
    /// returned.
    fn create_stack_checked(
        &self,
        input: CreateStackInput,
    ) -> Pin<Box<dyn Future<Output = Result<Stack, CreateStackCheckedError>> + '_>>;

    /// Create a stack and return a stream of subsequent stack events.
    ///
    /// This will call the `CreateStack` API to commence stack creation. If this returns
    /// successfully the `DescribeStackEvents` API is polled and the events are emitted through the
    /// the returned `Stream`. The stream ends when the stack reaches a settled state.
    ///
    /// # Errors
    ///
    /// This function itself will never return an error, however since any attempt to poll the
    /// `DescribeStackEvents` might fail, each event is wrapped in a `Result` and so must be checked
    /// for errors.
    ///
    /// Any errors returned when calling the `CreateStack` or `DescribeStackEvents` APIs are
    /// returned (via [`CreateStackStreamError::CreateStack`] and
    /// [`CreateStackStreamError::DescribeStackEvents`] respectively).
    ///
    /// If the stack settles with `ROLLBACK_COMPLETE` or `ROLLBACK_FAILED` status,
    /// [`CreateStackStreamError::Failed`] is returned.
    ///
    /// If the stack was seen with an unexpected status, [`CreateStackStreamError::Conflict`] is
    /// returned.
    fn create_stack_stream(
        &self,
        input: CreateStackInput,
    ) -> Pin<Box<dyn Stream<Item = Result<StackEvent, CreateStackStreamError>> + '_>>;

    /// Update a stack and wait for it to complete.
    ///
    /// This will call the `UpdateStack` API, but this only begins the update process. If
    /// `UpdateStack` returns successfully, the `DescribeStacks` API is polled until the stack has
    /// settled.
    ///
    /// # Errors
    ///
    /// Any errors returned when calling the `UpdateStack` or `DescribeStacks` APIs are returned
    /// (via [`UpdateStackCheckedError::UpdateStack`] and
    /// [`UpdateStackCheckedError::DescribeStacks`] respectively).
    ///
    /// If the stack settled with `UPDATE_ROLLBACK_COMPLETE` or `UPDATE_ROLLBACK_FAILED` status,
    /// [`UpdateStackCheckedError::Failed`] is returned.
    ///
    /// If the stack was seen with an unexpected status, [`UpdateStackCheckedError::Conflict`] is
    /// returned.
    fn update_stack_checked(
        &self,
        input: UpdateStackInput,
    ) -> Pin<Box<dyn Future<Output = Result<Stack, UpdateStackCheckedError>> + '_>>;

    /// Delete a stack and wait for the operation to complete.
    ///
    /// This will call the `DeleteStack` API, but this only begins the deletion process. If
    /// `DeleteStack` returns successfully, the `DescribeStacks` API is polled until the stack has
    /// settled.
    ///
    /// # Errors
    ///
    /// Any errors returned when calling the `DeleteStack` or `DescribeStacks` APIs are returned
    /// (via [`DeleteStackCheckedError::DeleteStack`] and
    /// [`DeleteStackCheckedError::DescribeStacks`] respectively).
    ///
    /// If the stack settled with `DELETE_FAILED` status, `DeleteStackCheckedError::Failed` is
    /// returned.
    ///
    /// If the stack was seen in an unexpected status, [`DeleteStackCheckedError::Conflict`] is
    /// returned.
    fn delete_stack_checked(
        &self,
        input: DeleteStackInput,
    ) -> Pin<Box<dyn Future<Output = Result<Stack, DeleteStackCheckedError>> + '_>>;

    /// Create a change set and wait for it to become available.
    ///
    /// This will call the `CreateChangeSet` API, but this only begins the creation process. If
    /// `CreateChangeSet` returns successfully, the `DescribeChangeSet` API is polled until the
    /// change set has settled.
    ///
    /// # Errors
    ///
    /// Any errors returned when calling the `CreateChangeSet` or `DescribeChangeSet` APIs are
    /// returned (via [`CreateChangeSetCheckedError::CreateChangeSet`] and
    /// [`CreateChangeSetCheckedError::DescribeChangeSet`] respectively).
    ///
    /// If the change set settled with a `FAILED` status, [`CreateChangeSetCheckedError::Failed`] is
    /// returned.
    ///
    /// If the change set was seen with an unexpected status,
    /// [`CreateChangeSetCheckedError::Conflict`] is returned.
    fn create_change_set_checked(
        &self,
        input: CreateChangeSetInput,
    ) -> Pin<
        Box<dyn Future<Output = Result<DescribeChangeSetOutput, CreateChangeSetCheckedError>> + '_>,
    >;
}

impl<T> CloudFormationExt for T
where
    T: CloudFormation,
{
    fn create_stack_checked(
        &self,
        input: CreateStackInput,
    ) -> Pin<Box<dyn Future<Output = Result<Stack, CreateStackCheckedError>> + '_>> {
        Box::pin(create_stack_checked(self, input))
    }

    fn create_stack_stream(
        &self,
        input: CreateStackInput,
    ) -> Pin<Box<dyn Stream<Item = Result<StackEvent, CreateStackStreamError>> + '_>> {
        Box::pin(create_stack_stream(self, input))
    }

    fn update_stack_checked(
        &self,
        input: UpdateStackInput,
    ) -> Pin<Box<dyn Future<Output = Result<Stack, UpdateStackCheckedError>> + '_>> {
        Box::pin(update_stack_checked(self, input))
    }

    fn delete_stack_checked(
        &self,
        input: DeleteStackInput,
    ) -> Pin<Box<dyn Future<Output = Result<Stack, DeleteStackCheckedError>> + '_>> {
        Box::pin(delete_stack_checked(self, input))
    }

    fn create_change_set_checked(
        &self,
        input: CreateChangeSetInput,
    ) -> Pin<
        Box<dyn Future<Output = Result<DescribeChangeSetOutput, CreateChangeSetCheckedError>> + '_>,
    > {
        Box::pin(create_change_set_checked(self, input))
    }
}

/// Errors that can occur during [`create_stack_checked`].
///
/// [`create_stack_checked`]: CloudFormationExt::create_stack_checked
#[derive(Debug, thiserror::Error)]
pub enum CreateStackCheckedError {
    /// The stack settled with a `ROLLBACK_COMPLETE` or `ROLLBACK_FAILED` status.
    #[error("stack failed to create; terminal status: {status}")]
    Failed { status: String, stack: Stack },

    /// The stack was modified while we waited for it to finish creating.
    #[error("stack had status {status} while waiting creation to finish")]
    Conflict { status: String, stack: Stack },

    /// The `CreateStack` operation returned an error.
    #[error("CreateStack error: {0}")]
    CreateStack(#[from] RusotoError<CreateStackError>),

    /// A `DescribeStacks` operation returned an error.
    #[error("DescribeStacks error: {0}")]
    DescribeStacks(#[from] RusotoError<DescribeStacksError>),
}

async fn create_stack_checked<Client: CloudFormation>(
    client: &Client,
    input: CreateStackInput,
) -> Result<Stack, CreateStackCheckedError> {
    let stack_id = client
        .create_stack(input)
        .await?
        .stack_id
        .expect("CreateStackOutput without stack_id");

    let describe_stacks_input = DescribeStacksInput {
        stack_name: Some(stack_id),
        ..DescribeStacksInput::default()
    };
    let mut interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    loop {
        interval.tick().await;

        let stack = client
            .describe_stacks(describe_stacks_input.clone())
            .await?
            .stacks
            .expect("DescribeStacksOutput without stacks")
            .pop()
            .expect("DescribeStacksOutput with empty stacks");
        match stack.stack_status.as_str() {
            "CREATE_IN_PROGRESS" | "CREATE_FAILED" | "ROLLBACK_IN_PROGRESS" => continue,
            "CREATE_COMPLETE" => return Ok(stack),
            "ROLLBACK_FAILED" | "ROLLBACK_COMPLETE" => {
                return Err(CreateStackCheckedError::Failed {
                    status: stack.stack_status.clone(),
                    stack,
                })
            }
            _ => {
                return Err(CreateStackCheckedError::Conflict {
                    status: stack.stack_status.clone(),
                    stack,
                })
            }
        }
    }
}

/// Errors that can be emitted by [`create_stack_stream`].
///
/// [`create_stack_stream`]: CloudFormationExt::create_stack_stream
#[derive(Debug, thiserror::Error)]
pub enum CreateStackStreamError {
    /// The stack settled with a `ROLLBACK_COMPLETE` or `ROLLBACK_FAILED` status.
    #[error("stack failed to create; terminal status: {status}")]
    Failed {
        status: String,
        stack_event: StackEvent,
    },

    /// The stack was modified while we waited for it to finish creating.
    #[error("stack had status {status} while waiting for creation to finish")]
    Conflict {
        status: String,
        stack_event: StackEvent,
    },

    /// The `CreateStack` operation returned an error.
    #[error("CreateStack error: {0}")]
    CreateStack(#[from] RusotoError<CreateStackError>),

    /// The `DescribeStackEvents` operation returned an error.
    #[error("DescribeStackEvents error: {0}")]
    DescribeStackEvents(#[from] RusotoError<DescribeStackEventsError>),
}

fn create_stack_stream<Client: CloudFormation>(
    client: &Client,
    input: CreateStackInput,
) -> impl Stream<Item = Result<StackEvent, CreateStackStreamError>> + '_ {
    try_stream! {
        let mut event_cutoff = format_timestamp(Utc::now());
        let stack_id = client
            .create_stack(input)
            .await?
            .stack_id
            .expect("CreateStackOutput without stack_id");

        let describe_stack_events_input = DescribeStackEventsInput {
            stack_name: Some(stack_id.clone()),
            ..DescribeStackEventsInput::default()
        };
        let mut interval = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
        );
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

            for stack_event in stack_events.into_iter().rev() {
                if stack_event.physical_resource_id.as_ref() != Some(&stack_id) {
                    yield stack_event;
                } else {
                    let stack_status = stack_event
                        .resource_status
                        .as_deref()
                        .expect("StackEvent without resource_status");
                    match stack_status {
                        "CREATE_IN_PROGRESS" | "CREATE_FAILED" | "ROLLBACK_IN_PROGRESS" => {
                            yield stack_event;
                        }
                        "CREATE_COMPLETE" => {
                            yield stack_event;
                            return;
                        }
                        "ROLLBACK_FAILED" | "ROLLBACK_COMPLETE" => {
                            Err(CreateStackStreamError::Failed {
                                status: stack_status.to_string(),
                                stack_event,
                            })?;
                            unreachable!()
                        }
                        _ => {
                            Err(CreateStackStreamError::Conflict {
                                status: stack_status.to_string(),
                                stack_event,
                            })?;
                            unreachable!()
                        }
                    }
                }
            }
        }
    }
}

/// Errors that can occur during [`update_stack_checked`].
///
/// [`update_stack_checked`]: CloudFormationExt::update_stack_checked
#[derive(Debug, thiserror::Error)]
pub enum UpdateStackCheckedError {
    /// The stack settled with a `UPDATE_ROLLBACK_COMPLETE` or `UPDATE_ROLLBACK_FAILED` status.
    #[error("stack failed to update; terminal status: {status}")]
    Failed { status: String, stack: Stack },

    /// The stack was modified while we waited for it to finish updating.
    #[error("stack had status {status} while waiting for update to finish")]
    Conflict { status: String, stack: Stack },

    /// The `UpdateStack` operation returned an error.
    #[error("UpdateStack error: {0}")]
    UpdateStack(#[from] RusotoError<UpdateStackError>),

    /// The `DescribeStacks` operation returned an error.
    #[error("DescribeStacks error: {0}")]
    DescribeStacks(#[from] RusotoError<DescribeStacksError>),
}

async fn update_stack_checked<Client: CloudFormation>(
    client: &Client,
    input: UpdateStackInput,
) -> Result<Stack, UpdateStackCheckedError> {
    let stack_id = client
        .update_stack(input)
        .await?
        .stack_id
        .expect("UpdateStackOutput without stack_id");

    let describe_stacks_input = DescribeStacksInput {
        stack_name: Some(stack_id),
        ..DescribeStacksInput::default()
    };
    let mut interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    loop {
        interval.tick().await;

        let stack = client
            .describe_stacks(describe_stacks_input.clone())
            .await?
            .stacks
            .expect("DescribeStacksOutput without stacks")
            .pop()
            .expect("DescribeStacksOutput with empty stacks");
        match stack.stack_status.as_str() {
            "UPDATE_IN_PROGRESS"
            | "UPDATE_COMPLETE_CLEANUP_IN_PROGRESS"
            | "UPDATE_ROLLBACK_IN_PROGRESS"
            | "UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS" => continue,
            "UPDATE_COMPLETE" => return Ok(stack),
            "UPDATE_ROLLBACK_FAILED" | "UPDATE_ROLLBACK_COMPLETE" => {
                return Err(UpdateStackCheckedError::Failed {
                    status: stack.stack_status.clone(),
                    stack,
                })
            }
            _ => {
                return Err(UpdateStackCheckedError::Conflict {
                    status: stack.stack_status.clone(),
                    stack,
                })
            }
        }
    }
}

/// Errors that can occur during [`delete_stack_checked`].
///
/// [`delete_stack_checked`]: CloudFormationExt::delete_stack_checked
#[derive(Debug, thiserror::Error)]
pub enum DeleteStackCheckedError {
    /// The stack settled with `DELETE_COMPLETE` status.
    #[error("stack failed to delete; terminal status: {status}")]
    Failed { status: String, stack: Stack },

    /// The stack was modified while we waited for the deletion to finish.
    #[error("stack had status {status} while waiting for deletion to finish")]
    Conflict { status: String, stack: Stack },

    /// The `DeleteStack` operation returned an error.
    #[error("DeleteStack error: {0}")]
    DeleteStack(#[from] RusotoError<DeleteStackError>),

    /// The `DescribeStacks` operation returned an error.
    #[error("DescribeStacks error: {0}")]
    DescribeStacks(#[from] RusotoError<DescribeStacksError>),
}

async fn delete_stack_checked<Client: CloudFormation>(
    client: &Client,
    input: DeleteStackInput,
) -> Result<Stack, DeleteStackCheckedError> {
    let describe_stacks_input = DescribeStacksInput {
        stack_name: Some(input.stack_name.clone()),
        ..DescribeStacksInput::default()
    };
    if let Some(stack) = client
        .describe_stacks(describe_stacks_input)
        .await?
        .stacks
        .expect("DescribeStacksOutput without stacks")
        .pop()
    {
        let stack_id = stack.stack_id.expect("Stack without stack_id");

        client.delete_stack(input).await?;

        let describe_stacks_input = DescribeStacksInput {
            stack_name: Some(stack_id),
            ..DescribeStacksInput::default()
        };
        let mut interval = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
        );
        loop {
            interval.tick().await;

            let stack = client
                .describe_stacks(describe_stacks_input.clone())
                .await?
                .stacks
                .expect("DescribeStacksOutput without stacks")
                .pop()
                .expect("DescribeStacksOutput with empty stacks");
            match stack.stack_status.as_str() {
                "DELETE_IN_PROGRESS" => continue,
                "DELETE_COMPLETE" => return Ok(stack),
                "DELETE_FAILED" => {
                    return Err(DeleteStackCheckedError::Failed {
                        status: stack.stack_status.clone(),
                        stack,
                    })
                }
                _ => {
                    return Err(DeleteStackCheckedError::Conflict {
                        status: stack.stack_status.clone(),
                        stack,
                    })
                }
            }
        }
    } else {
        // The stack doesn't seem to exist, but we'll let the `DeleteStack` API handle this.
        client.delete_stack(input).await?;

        panic!("delete_stack_checked succeeded even though stack doesn't exist");
    }
}

/// Errors that can occur during [`create_change_set_checked`].
///
/// [`create_change_set_checked`]: CloudFormationExt::create_change_set_checked
#[derive(Debug, thiserror::Error)]
pub enum CreateChangeSetCheckedError {
    /// The change set settled with a `FAILED` status.
    #[error("change set failed to create; terminal status: {status}")]
    Failed {
        status: String,
        change_set: DescribeChangeSetOutput,
    },

    /// The change set was modified while we waited for it to become available.
    #[error("change set had status {status} while waiting for it to create")]
    Conflict {
        status: String,
        change_set: DescribeChangeSetOutput,
    },

    /// The `CreateChangeSet` operation returned an error.
    #[error("CreateChangeSet error: {0}")]
    CreateChangeSet(#[from] RusotoError<CreateChangeSetError>),

    /// A `DescribeChangeSet` operation returned an error.
    #[error("DescribeChangeSet error: {0}")]
    DescribeChangeSet(#[from] RusotoError<DescribeChangeSetError>),
}

async fn create_change_set_checked<Client: CloudFormation>(
    client: &Client,
    input: CreateChangeSetInput,
) -> Result<DescribeChangeSetOutput, CreateChangeSetCheckedError> {
    let change_set_id = client
        .create_change_set(input)
        .await?
        .id
        .expect("CreateChangeSetOutput without id");

    let describe_change_set_input = DescribeChangeSetInput {
        change_set_name: change_set_id,
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
        match change_set_status {
            "CREATE_PENDING" | "CREATE_IN_PROGRESS" => continue,
            "CREATE_COMPLETE" => return Ok(change_set),
            "FAILED" => {
                return Err(CreateChangeSetCheckedError::Failed {
                    status: change_set_status.to_string(),
                    change_set,
                })
            }
            _ => {
                return Err(CreateChangeSetCheckedError::Conflict {
                    status: change_set_status.to_string(),
                    change_set,
                })
            }
        }
    }
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
