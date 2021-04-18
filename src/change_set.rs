//! Helpers for working with change sets.

use std::{future::Future, time::Duration};

use futures_util::TryFutureExt;
use memmem::{Searcher, TwoWaySearcher};
use rusoto_cloudformation::{
    CloudFormation, CreateChangeSetError, CreateChangeSetInput, DescribeChangeSetError,
    DescribeChangeSetInput, DescribeChangeSetOutput,
};
use rusoto_core::{request::BufferedHttpResponse, RusotoError};
use tokio::time::{interval_at, Instant};

use crate::status::ChangeSetStatus;

const CHANGE_SET_POLL_INTERVAL: Duration = Duration::from_secs(1);

type CreateChangeSetResult = Result<
    // The nested `Result` is intended to make it hard to ignore the status of the resulting change
    // set and going on to, e.g., try to execute a failed change set. The `Option` indicates the
    // case where creation failed due to no changes being present.
    Result<ChangeSet, ChangeSet>,
    RusotoError<DescribeChangeSetError>,
>;

pub(crate) struct ChangeSet {
    pub(crate) id: String,
    pub(crate) stack_id: String,
    pub(crate) status: ChangeSetStatus,
    pub(crate) status_reason: Option<String>,
}

impl ChangeSet {
    fn from_raw(change_set: DescribeChangeSetOutput) -> Self {
        Self {
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

pub(crate) async fn create_change_set<Client: CloudFormation>(
    client: &Client,
    mut input: CreateChangeSetInput,
) -> Result<impl Future<Output = CreateChangeSetResult> + '_, RusotoError<CreateChangeSetError>> {
    let is_create = input.change_set_type.as_deref() == Some("CREATE");
    let change_set = client.create_change_set(input.clone());
    let change_set = if is_create {
        change_set
            .or_else(|error| async move {
                match error {
                    RusotoError::Unknown(ref response) if is_already_exists(response) => {
                        input.change_set_type = Some("UPDATE".to_string());
                        client.create_change_set(input).await
                    }
                    error => Err(error),
                }
            })
            .await?
    } else {
        change_set.await?
    };
    let change_set_id = change_set.id.expect("CreateChangeSetOutput without id");

    let mut interval = interval_at(
        Instant::now() + CHANGE_SET_POLL_INTERVAL,
        CHANGE_SET_POLL_INTERVAL,
    );
    let describe_change_set_input = DescribeChangeSetInput {
        change_set_name: change_set_id,
        ..DescribeChangeSetInput::default()
    };
    Ok(async move {
        loop {
            interval.tick().await;

            let change_set = client
                .describe_change_set(describe_change_set_input.clone())
                .await?;
            let change_set = ChangeSet::from_raw(change_set);
            match change_set.status {
                ChangeSetStatus::CreatePending | ChangeSetStatus::CreateInProgress => continue,
                ChangeSetStatus::CreateComplete => return Ok(Ok(change_set)),
                ChangeSetStatus::Failed => return Ok(Err(change_set)),
                _ => {
                    panic!(
                        "change set {} had unexpected status: {}",
                        change_set.id, change_set.status
                    );
                }
            }
        }
    })
}

fn is_already_exists(response: &BufferedHttpResponse) -> bool {
    TwoWaySearcher::new(b" already exists ")
        .search_in(&response.body)
        .is_some()
}
