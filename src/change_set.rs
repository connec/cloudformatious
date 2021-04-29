//! Helpers for working with change sets.

use std::{fmt, time::Duration};

use chrono::Utc;
use futures_util::TryFutureExt;
use memmem::{Searcher, TwoWaySearcher};
use rusoto_cloudformation::{
    CloudFormation, CreateChangeSetInput, DescribeChangeSetError, DescribeChangeSetInput,
    DescribeChangeSetOutput, ExecuteChangeSetError, ExecuteChangeSetInput,
};
use rusoto_core::{request::BufferedHttpResponse, RusotoError};
use tokio::time::{interval_at, Instant};

use crate::{
    stack::{StackOperation, StackOperationStatus},
    status::{ChangeSetStatus, StackStatus},
};

const POLL_INTERVAL_CHANGE_SET: Duration = Duration::from_secs(1);

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

pub(crate) async fn execute_change_set<Client: CloudFormation>(
    client: &Client,
    change_set: ChangeSet,
) -> Result<
    StackOperation<'_, impl Fn(StackStatus) -> StackOperationStatus + Unpin>,
    RusotoError<ExecuteChangeSetError>,
> {
    let started_at = Utc::now();
    let input = ExecuteChangeSetInput {
        change_set_name: change_set.id,
        ..ExecuteChangeSetInput::default()
    };
    client.execute_change_set(input).await?;

    Ok(StackOperation::new(
        client,
        change_set.stack_id,
        started_at,
        match change_set.r#type {
            ChangeSetType::Create => check_create_progress,
            ChangeSetType::Update => check_update_progress,
        },
    ))
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

fn check_create_progress(stack_status: StackStatus) -> StackOperationStatus {
    match stack_status {
        StackStatus::CreateInProgress | StackStatus::RollbackInProgress => {
            StackOperationStatus::InProgress
        }
        StackStatus::CreateComplete => StackOperationStatus::Complete,
        StackStatus::CreateFailed | StackStatus::RollbackFailed | StackStatus::RollbackComplete => {
            StackOperationStatus::Failed
        }
        _ => StackOperationStatus::Unexpected,
    }
}

fn check_update_progress(stack_status: StackStatus) -> StackOperationStatus {
    match stack_status {
        StackStatus::UpdateInProgress
        | StackStatus::UpdateCompleteCleanupInProgress
        | StackStatus::UpdateRollbackInProgress
        | StackStatus::UpdateRollbackCompleteCleanupInProgress => StackOperationStatus::InProgress,
        StackStatus::UpdateComplete => StackOperationStatus::Complete,
        StackStatus::UpdateRollbackFailed | StackStatus::UpdateRollbackComplete => {
            StackOperationStatus::Failed
        }
        _ => StackOperationStatus::Unexpected,
    }
}
