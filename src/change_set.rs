//! Helpers for working with change sets.

use std::{fmt, time::Duration};

use async_stream::try_stream;
use chrono::Utc;
use futures_util::Stream;
use futures_util::TryFutureExt;
use memmem::{Searcher, TwoWaySearcher};
use rusoto_cloudformation::{
    CloudFormation, CreateChangeSetInput, DescribeChangeSetError, DescribeChangeSetInput,
    DescribeChangeSetOutput, DescribeStackEventsError, DescribeStackEventsInput,
    ExecuteChangeSetInput,
};
use rusoto_core::{request::BufferedHttpResponse, RusotoError};
use tokio::time::{interval_at, Instant};

use crate::{
    event::StackEvent,
    status::{ChangeSetStatus, StackStatus},
};

const POLL_INTERVAL_CHANGE_SET: Duration = Duration::from_secs(1);
const POLL_INTERVAL_STACK_EVENT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeSetType {
    Create,
    Update,
}

impl ChangeSetType {
    fn try_from(change_set_type: Option<&str>) -> Result<Self, String> {
        match change_set_type {
            Some("CREATE") => Ok(Self::Create),
            None | Some("UPDATE") => Ok(Self::Update),
            Some(other) => Err(other.to_string()),
        }
    }
}

impl fmt::Display for ChangeSetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => write!(f, "CREATE"),
            Self::Update => write!(f, "UPDATE"),
        }
    }
}

pub(crate) struct ChangeSet {
    pub(crate) r#type: ChangeSetType,
    pub(crate) id: String,
    pub(crate) stack_id: String,
    pub(crate) status: ChangeSetStatus,
    pub(crate) status_reason: Option<String>,
}

impl ChangeSet {
    fn from_raw(r#type: ChangeSetType, change_set: DescribeChangeSetOutput) -> Self {
        Self {
            r#type,
            id: change_set
                .change_set_id
                .expect("DescribeChangeSetOutput without change_set_id"),
            stack_id: change_set
                .stack_id
                .expect("DescribeChangeSetOutput without stack_id"),
            status: change_set
                .status
                .expect("DescribeChangeSetOutput without status")
                .parse()
                .expect("DescribeChangeSetOutput unexpected status"),
            status_reason: change_set.status_reason,
        }
    }
}

pub(crate) enum CreateChangeSetError {
    CreateApi(RusotoError<rusoto_cloudformation::CreateChangeSetError>),
    PollApi(RusotoError<DescribeChangeSetError>),
    NoChanges(ChangeSet),
    Failed(ChangeSet),
}

impl From<RusotoError<rusoto_cloudformation::CreateChangeSetError>> for CreateChangeSetError {
    fn from(error: RusotoError<rusoto_cloudformation::CreateChangeSetError>) -> Self {
        Self::CreateApi(error)
    }
}

impl From<RusotoError<DescribeChangeSetError>> for CreateChangeSetError {
    fn from(error: RusotoError<DescribeChangeSetError>) -> Self {
        Self::PollApi(error)
    }
}

pub(crate) async fn create_change_set<Client: CloudFormation>(
    client: &Client,
    mut input: CreateChangeSetInput,
) -> Result<ChangeSet, CreateChangeSetError> {
    let mut change_set_type = ChangeSetType::try_from(input.change_set_type.as_deref());
    let change_set = client.create_change_set(input.clone());
    let change_set = change_set
        .or_else({
            let change_set_type = &mut change_set_type;
            |error| async move {
                match (change_set_type, error) {
                    (
                        Ok(change_set_type @ ChangeSetType::Create),
                        RusotoError::Unknown(ref response),
                    ) if is_already_exists(response) => {
                        *change_set_type = ChangeSetType::Update;
                        input.change_set_type = Some(change_set_type.to_string());
                        client.create_change_set(input).await
                    }
                    (_, error) => Err(error),
                }
            }
        })
        .await?;
    let change_set_type =
        change_set_type.expect("CreateChangeSet succeeded with invalid change_set_type");
    let change_set_id = change_set.id.expect("CreateChangeSetOutput without id");

    let mut interval = interval_at(
        Instant::now() + POLL_INTERVAL_CHANGE_SET,
        POLL_INTERVAL_CHANGE_SET,
    );
    let describe_change_set_input = DescribeChangeSetInput {
        change_set_name: change_set_id,
        ..DescribeChangeSetInput::default()
    };
    loop {
        interval.tick().await;

        let change_set = client
            .describe_change_set(describe_change_set_input.clone())
            .await?;
        let change_set = ChangeSet::from_raw(change_set_type, change_set);
        match change_set.status {
            ChangeSetStatus::CreatePending | ChangeSetStatus::CreateInProgress => continue,
            ChangeSetStatus::CreateComplete => return Ok(change_set),
            ChangeSetStatus::Failed if is_no_changes(change_set.status_reason.as_deref()) => {
                return Err(CreateChangeSetError::NoChanges(change_set))
            }
            ChangeSetStatus::Failed => return Err(CreateChangeSetError::Failed(change_set)),
            _ => {
                panic!(
                    "change set {} had unexpected status: {}",
                    change_set.id, change_set.status
                );
            }
        }
    }
}

pub(crate) enum ExecuteChangeSetError {
    ExecuteApi(RusotoError<rusoto_cloudformation::ExecuteChangeSetError>),
    PollApi(RusotoError<DescribeStackEventsError>),
    Failed {
        // Separate status from event to avoid having to match
        status: StackStatus,
        event: StackEvent,
    },
}

impl From<RusotoError<rusoto_cloudformation::ExecuteChangeSetError>> for ExecuteChangeSetError {
    fn from(error: RusotoError<rusoto_cloudformation::ExecuteChangeSetError>) -> Self {
        Self::ExecuteApi(error)
    }
}

impl From<RusotoError<DescribeStackEventsError>> for ExecuteChangeSetError {
    fn from(error: RusotoError<DescribeStackEventsError>) -> Self {
        Self::PollApi(error)
    }
}

/// Private enum to simplify stack event handling in [`execute_change_set`].
enum ExecuteStatus {
    InProgress,
    Complete,
    Failed,
    Unexpected,
}

pub(crate) fn execute_change_set<Client: CloudFormation>(
    client: &Client,
    change_set: ChangeSet,
) -> impl Stream<Item = Result<StackEvent, ExecuteChangeSetError>> + '_ {
    try_stream! {
        let mut since = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let input = ExecuteChangeSetInput {
            change_set_name: change_set.id,
            ..ExecuteChangeSetInput::default()
        };
        client.execute_change_set(input).await?;

        let mut interval = tokio::time::interval(POLL_INTERVAL_STACK_EVENT);
        let describe_stack_events_input = DescribeStackEventsInput {
            stack_name: Some(change_set.stack_id),
            ..DescribeStackEventsInput::default()
        };

        let change_set_type = change_set.r#type;
        loop {
            interval.tick().await;

            let stack_events: Vec<_> = client
                .describe_stack_events(describe_stack_events_input.clone())
                .await?
                .stack_events
                .expect("DescribeStackEventsOutput without stack_events")
                .into_iter()
                .take_while(|event| event.timestamp > since)
                .map(StackEvent::from_raw)
                .collect();

            if let Some(stack_event) = stack_events.first() {
                since = stack_event
                    .timestamp()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            }

            for stack_event in stack_events.into_iter().rev() {
                match &stack_event {
                    StackEvent::Resource { .. } => yield stack_event,
                    StackEvent::Stack {
                        resource_status, ..
                    } => match get_execute_status(change_set_type, *resource_status) {
                        ExecuteStatus::InProgress => yield stack_event,
                        ExecuteStatus::Complete => {
                            yield stack_event;
                            return;
                        }
                        ExecuteStatus::Failed => {
                            Err(ExecuteChangeSetError::Failed {
                                status: *resource_status,
                                event: stack_event,
                            })?;
                            unreachable!()
                        }
                        ExecuteStatus::Unexpected => {
                            panic!(
                                "stack {} has unexpected status for {}: {}",
                                describe_stack_events_input
                                    .stack_name
                                    .as_deref()
                                    .unwrap_or(""),
                                change_set_type,
                                resource_status
                            );
                        }
                    },
                }
            }
        }
    }
}

fn is_already_exists(response: &BufferedHttpResponse) -> bool {
    TwoWaySearcher::new(b" already exists ")
        .search_in(&response.body)
        .is_some()
}

fn is_no_changes(status_reason: Option<&str>) -> bool {
    status_reason
        .unwrap_or_default()
        .contains("The submitted information didn't contain changes.")
}

fn get_execute_status(change_set_type: ChangeSetType, stack_status: StackStatus) -> ExecuteStatus {
    match change_set_type {
        ChangeSetType::Create => match stack_status {
            StackStatus::CreateInProgress | StackStatus::RollbackInProgress => {
                ExecuteStatus::InProgress
            }
            StackStatus::CreateComplete => ExecuteStatus::Complete,
            StackStatus::CreateFailed
            | StackStatus::RollbackFailed
            | StackStatus::RollbackComplete => ExecuteStatus::Failed,
            _ => ExecuteStatus::Unexpected,
        },
        ChangeSetType::Update => match stack_status {
            StackStatus::UpdateInProgress
            | StackStatus::UpdateCompleteCleanupInProgress
            | StackStatus::UpdateRollbackInProgress
            | StackStatus::UpdateRollbackCompleteCleanupInProgress => ExecuteStatus::InProgress,
            StackStatus::UpdateComplete => ExecuteStatus::Complete,
            StackStatus::UpdateRollbackFailed | StackStatus::UpdateRollbackComplete => {
                ExecuteStatus::Failed
            }
            _ => ExecuteStatus::Unexpected,
        },
    }
}
