use std::{fmt, future::Future, pin::Pin, task};

use async_stream::try_stream;
use chrono::Utc;
use futures_util::{Stream, TryStreamExt};
use memmem::{Searcher, TwoWaySearcher};
use rusoto_cloudformation::{CloudFormation, DescribeStacksInput};
use rusoto_core::{request::BufferedHttpResponse, RusotoError};

use crate::{
    stack::{StackOperation, StackOperationError, StackOperationStatus},
    StackEvent, StackFailure, StackStatus, StackWarning,
};

/// The input for the `delete_stack` operation.
///
/// You can create a delete stack input via the [`new`](Self::new) associated function. Setters are
/// also available to make constructing sparse inputs more ergonomic.
///
/// ```no_run
/// # use rusoto_cloudformation::CloudFormationClient;
/// # use rusoto_core::Region;
/// use cloudformatious::{CloudFormatious, DeleteStackInput};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = CloudFormationClient::new(Region::EuWest2);
/// let input = DeleteStackInput::new("my-stack")
///     .set_client_request_token("hello")
///     .set_retain_resources(["MyResource"])
///     .set_role_arn("arn:foo");
/// client.delete_stack(input).await?;
/// // ...
/// # Ok(())
/// # }
/// ```
#[allow(clippy::module_name_repetitions)]
pub struct DeleteStackInput {
    /// A unique identifier for this `DeleteStack` request. Specify this token if you plan to retry
    /// requests so that AWS CloudFormation knows that you're not attempting to delete a stack with
    /// the same name. You might retry `DeleteStack` requests to ensure that AWS CloudFormation
    /// successfully received them.
    ///
    /// All events triggered by a given stack operation are assigned the same client request token,
    /// which you can use to track operations. For example, if you execute a `CreateStack` operation
    /// with the token `token1`, then all the `StackEvent`s generated by that operation will have
    /// `ClientRequestToken` set as `token1`.
    ///
    /// In the console, stack operations display the client request token on the Events tab. Stack
    /// operations that are initiated from the console use the token format
    /// `Console-StackOperation-ID`, which helps you easily identify the stack operation . For
    /// example, if you create a stack using the console, each stack event would be assigned the
    /// same token in the following format:
    /// `Console-CreateStack-7f59c3cf-00d2-40c7-b2ff-e75db0987002`.
    pub client_request_token: Option<String>,

    /// For stacks in the `DELETE_FAILED` state, a list of resource logical IDs that are associated
    /// with the resources you want to retain. During deletion, AWS CloudFormation deletes the stack
    /// but does not delete the retained resources.
    ///
    /// Retaining resources is useful when you cannot delete a resource, such as a non-empty S3
    /// bucket, but you want to delete the stack.
    pub retain_resources: Option<Vec<String>>,

    /// The Amazon Resource Name (ARN) of an AWS Identity and Access Management (IAM) role that AWS
    /// CloudFormation assumes to delete the stack. AWS CloudFormation uses the role's credentials
    /// to make calls on your behalf.
    ///
    /// If you don't specify a value, AWS CloudFormation uses the role that was previously
    /// associated with the stack. If no role is available, AWS CloudFormation uses a temporary
    /// session that is generated from your user credentials.
    pub role_arn: Option<String>,

    /// The name or the unique stack ID that is associated with the stack.
    pub stack_name: String,
}

impl DeleteStackInput {
    /// Construct an input for the given `stack_name` and `template_source`.
    pub fn new(stack_name: impl Into<String>) -> Self {
        Self {
            stack_name: stack_name.into(),

            client_request_token: None,
            retain_resources: None,
            role_arn: None,
        }
    }

    /// Set the value for `client_request_token`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_client_request_token(mut self, client_request_token: impl Into<String>) -> Self {
        self.client_request_token = Some(client_request_token.into());
        self
    }

    /// Set the value for `client_request_token`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_retain_resources<I, S>(mut self, retain_resources: I) -> Self
    where
        I: Into<Vec<S>>,
        S: Into<String>,
    {
        self.retain_resources = Some(
            retain_resources
                .into()
                .into_iter()
                .map(Into::into)
                .collect(),
        );
        self
    }

    /// Set the value for `role_arn`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_role_arn(mut self, role_arn: impl Into<String>) -> Self {
        self.role_arn = Some(role_arn.into());
        self
    }

    fn into_raw(self) -> rusoto_cloudformation::DeleteStackInput {
        rusoto_cloudformation::DeleteStackInput {
            client_request_token: self.client_request_token,
            retain_resources: self.retain_resources,
            role_arn: self.role_arn,
            stack_name: self.stack_name,
        }
    }
}

/// Errors emitted by a `delete_stack` operation.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub enum DeleteStackError {
    /// A CloudFormation API error occurred.
    ///
    /// This is likely to be due to invalid input parameters or missing CloudFormation permissions.
    /// The inner error should have a descriptive message.
    ///
    /// **Note:** the inner error will always be some variant of [`RusotoError`], but since they are
    /// generic over the type of service errors we either need a variant per API used, or `Box`. If
    /// you do need to programmatically match a particular API error you can use [`Box::downcast`].
    CloudFormationApi(Box<dyn std::error::Error>),

    /// The delete stack operation failed.
    Failure(StackFailure),

    /// The delete stack operation succeeded with warnings.
    Warning(StackWarning),
}

impl DeleteStackError {
    fn from_rusoto_error<E: std::error::Error + 'static>(error: RusotoError<E>) -> Self {
        Self::CloudFormationApi(error.into())
    }
}

impl fmt::Display for DeleteStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CloudFormationApi(error) => {
                write!(f, "CloudFormation API error: {}", error)
            }
            Self::Failure(failure) => write!(f, "{}", failure),
            Self::Warning(warning) => write!(f, "{}", warning),
        }
    }
}

impl std::error::Error for DeleteStackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CloudFormationApi(error) => Some(error.as_ref()),
            Self::Failure { .. } | Self::Warning { .. } => None,
        }
    }
}

/// An ongoing `delete_stack` operation.
///
/// This implements `Future`, which will simply wait for the operation to conclude. If you want to
/// observe progress, see [`DeleteStack::events`].
pub struct DeleteStack<'client> {
    event_stream: Pin<Box<dyn Stream<Item = Result<StackEvent, DeleteStackError>> + 'client>>,
    output: Option<Result<(), DeleteStackError>>,
}

impl<'client> DeleteStack<'client> {
    pub(crate) fn new<Client: CloudFormation>(
        client: &'client Client,
        input: DeleteStackInput,
    ) -> Self {
        let event_stream = try_stream! {
            let stack_id = match describe_stack_id(client, input.stack_name.clone()).await? {
                Some(stack_id) => stack_id,
                None => return,
            };

            let started_at = Utc::now();
            client
                .delete_stack(input.into_raw())
                .await
                .map_err(DeleteStackError::from_rusoto_error)?;

            let mut operation =
                StackOperation::new(client, stack_id, started_at, check_operation_status);
            while let Some(event) = operation
                .try_next()
                .await
                .map_err(DeleteStackError::from_rusoto_error)?
            {
                yield event;
            }

            match operation.verify() {
                Ok(()) => {}
                Err(StackOperationError::Failure(failure)) => {
                    Err(DeleteStackError::Failure(failure))?;
                    unreachable!()
                }
                Err(StackOperationError::Warning(warning)) => {
                    Err(DeleteStackError::Warning(warning))?;
                    unreachable!()
                }
            };
        };
        Self {
            event_stream: Box::pin(event_stream),
            output: None,
        }
    }

    /// Get a `Stream` of `StackEvent`s.
    pub fn events(&mut self) -> DeleteStackEvents<'client, '_> {
        DeleteStackEvents(self)
    }

    fn poll_next_internal(&mut self, ctx: &mut task::Context) -> task::Poll<Option<StackEvent>> {
        match self.event_stream.as_mut().poll_next(ctx) {
            task::Poll::Pending => task::Poll::Pending,
            task::Poll::Ready(None) => {
                self.output.get_or_insert(Ok(()));
                task::Poll::Ready(None)
            }
            task::Poll::Ready(Some(Ok(event))) => task::Poll::Ready(Some(event)),
            task::Poll::Ready(Some(Err(error))) => {
                self.output.replace(Err(error));
                task::Poll::Ready(None)
            }
        }
    }
}

impl Future for DeleteStack<'_> {
    type Output = Result<(), DeleteStackError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Self::Output> {
        loop {
            match self.poll_next_internal(ctx) {
                task::Poll::Pending => return task::Poll::Pending,
                task::Poll::Ready(None) => {
                    return task::Poll::Ready(
                        self.output
                            .take()
                            .expect("end of stream without err or output"),
                    )
                }
                task::Poll::Ready(Some(_)) => continue,
            }
        }
    }
}

/// Return value of [`DeleteStack::events`].
#[allow(clippy::module_name_repetitions)]
pub struct DeleteStackEvents<'client, 'delete>(&'delete mut DeleteStack<'client>);

impl Stream for DeleteStackEvents<'_, '_> {
    type Item = StackEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut task::Context,
    ) -> task::Poll<Option<Self::Item>> {
        self.0.poll_next_internal(ctx)
    }
}

async fn describe_stack_id<Client: CloudFormation>(
    client: &Client,
    stack_name: String,
) -> Result<Option<String>, DeleteStackError> {
    let describe_stacks_input = DescribeStacksInput {
        stack_name: Some(stack_name),
        ..DescribeStacksInput::default()
    };

    let output = match client.describe_stacks(describe_stacks_input).await {
        Ok(output) => output,
        Err(RusotoError::Unknown(response)) if is_not_exists(&response) => return Ok(None),
        Err(error) => return Err(DeleteStackError::from_rusoto_error(error)),
    };

    let stack = output
        .stacks
        .expect("DescribeStacksOutput without stacks")
        .pop()
        .expect("DescribeStacksOutput empty stacks");

    if stack.stack_status.parse() == Ok(StackStatus::DeleteComplete) {
        Ok(None)
    } else {
        Ok(Some(stack.stack_id.expect("Stack without stack_id")))
    }
}

fn is_not_exists(response: &BufferedHttpResponse) -> bool {
    TwoWaySearcher::new(b"does not exist")
        .search_in(&response.body)
        .is_some()
}

fn check_operation_status(stack_status: StackStatus) -> StackOperationStatus {
    match stack_status {
        StackStatus::DeleteInProgress => StackOperationStatus::InProgress,
        StackStatus::DeleteComplete => StackOperationStatus::Complete,
        StackStatus::DeleteFailed => StackOperationStatus::Failed,
        _ => StackOperationStatus::Unexpected,
    }
}
