use std::{fmt, pin::Pin, task, time::Duration};

use async_stream::try_stream;
use chrono::{DateTime, Utc};
use futures_util::Stream;
use rusoto_cloudformation::{CloudFormation, DescribeStackEventsError, DescribeStackEventsInput};
use rusoto_core::RusotoError;

use crate::{ResourceStatus, StackEvent, StackEventDetails, StackStatus, Status};

const POLL_INTERVAL_STACK_EVENT: Duration = Duration::from_secs(5);

/// Describes a failed stack operation.
///
/// This error tries to capture enough information to quickly identify the root-cause of the
/// operation's failure (such as not having permission to create or update a particular resource
/// in the stack).
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct StackFailure {
    /// The ID of the stack.
    pub stack_id: String,

    /// The failed status in which the stack settled.
    pub stack_status: StackStatus,

    /// The *first* reason the stack moved into a failing state.
    ///
    /// Note that this may not be the reason associated with the current `stack_status`, but rather
    /// the reason for the first negative status the stack entered (which is usually more
    /// descriptive).
    pub stack_status_reason: String,

    /// Resource events with negative statuses that may have precipitated the failure of the
    /// operation.
    ///
    /// **Note:** this is represented as a `Vec` or tuples to avoid having to worry about
    /// matching [`StackEvent`] variants (when it would be a logical error for them to be
    /// anything other than the `Resource` variant).
    pub resource_events: Vec<(ResourceStatus, StackEventDetails)>,
}

impl fmt::Display for StackFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Stack operation failed for {}; terminal status: {} ({})",
            self.stack_id, self.stack_status, self.stack_status_reason
        )?;

        if !self.resource_events.is_empty() {
            writeln!(f, "\nThe following resources had errors:")?;
        }
        for (resource_status, details) in &self.resource_events {
            write!(
                f,
                "\n- {} ({}): {} ({})",
                details.logical_resource_id,
                details.resource_type,
                resource_status,
                details
                    .resource_status_reason
                    .as_deref()
                    .unwrap_or("no reason reported"),
            )?;
        }

        Ok(())
    }
}

/// Describes a successful stack operation with warnings.
///
/// It is possible for resource errors to occur even when the overall operation succeeds, such
/// as failing to delete a resource during clean-up after a successful update. Rather than
/// letting this pass silently, or relying on carefully interrogating `StackEvent`s, the
/// operation returns an error.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct StackWarning {
    /// The ID of the stack.
    pub stack_id: String,

    /// Resource events with negative statuses that did not affect the overall operation.
    ///
    /// **Note:** this is represented as a `Vec` of tuples to avoid having to worry about
    /// matching [`StackEvent`] variants (when it would be a logical error for them to be
    /// anything other than the `Resource` variant).
    pub resource_events: Vec<(ResourceStatus, StackEventDetails)>,
}

impl fmt::Display for StackWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Stack {} applied successfully but some resources had errors:",
            self.stack_id
        )?;
        for (resource_status, details) in &self.resource_events {
            write!(
                f,
                "\n- {} ({}): {} ({})",
                details.logical_resource_id,
                details.resource_type,
                resource_status,
                details
                    .resource_status_reason
                    .as_deref()
                    .unwrap_or("no reason reported")
            )?;
        }
        Ok(())
    }
}

pub(crate) enum StackOperationError {
    Failure(StackFailure),
    Warning(StackWarning),
}

pub(crate) enum StackOperationStatus {
    InProgress,
    Complete,
    Failed,
    Unexpected,
}

pub(crate) struct StackOperation<'client, F> {
    stack_id: String,
    check_progress: F,
    events: Pin<
        Box<dyn Stream<Item = Result<StackEvent, RusotoError<DescribeStackEventsError>>> + 'client>,
    >,
    stack_error_status: Option<StackStatus>,
    stack_error_status_reason: Option<String>,
    resource_error_events: Vec<(ResourceStatus, StackEventDetails)>,
}

impl<'client, F> StackOperation<'client, F>
where
    F: Fn(StackStatus) -> StackOperationStatus + Unpin,
{
    pub(crate) fn new<Client: CloudFormation>(
        client: &'client Client,
        stack_id: String,
        started_at: DateTime<Utc>,
        check_progress: F,
    ) -> Self {
        let describe_stack_events_input = DescribeStackEventsInput {
            stack_name: Some(stack_id.clone()),
            ..DescribeStackEventsInput::default()
        };
        let events = try_stream! {
            let mut interval = tokio::time::interval(POLL_INTERVAL_STACK_EVENT);
            let mut since = started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

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
                    let is_terminal = stack_event.is_terminal();

                    yield stack_event;

                    if is_terminal {
                        return;
                    }
                }
            }
        };
        Self {
            stack_id,
            check_progress,
            events: Box::pin(events),
            stack_error_status: None,
            stack_error_status_reason: None,
            resource_error_events: Vec::new(),
        }
    }

    pub(crate) async fn verify(self) -> Result<(), StackOperationError> {
        if let Some(stack_status) = self.stack_error_status {
            return Err(StackOperationError::Failure(StackFailure {
                stack_id: self.stack_id,
                stack_status,
                stack_status_reason: self
                    .stack_error_status_reason
                    .expect("stack op failed with no reasons"),
                resource_events: self.resource_error_events,
            }));
        }

        if self.resource_error_events.is_empty() {
            Ok(())
        } else {
            Err(StackOperationError::Warning(StackWarning {
                stack_id: self.stack_id,
                resource_events: self.resource_error_events,
            }))
        }
    }
}

impl<F> Stream for StackOperation<'_, F>
where
    F: Fn(StackStatus) -> StackOperationStatus + Unpin,
{
    type Item = Result<StackEvent, RusotoError<DescribeStackEventsError>>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        match self.events.as_mut().poll_next(ctx) {
            task::Poll::Pending => task::Poll::Pending,
            task::Poll::Ready(None) => task::Poll::Ready(None),
            task::Poll::Ready(Some(Err(error))) => task::Poll::Ready(Some(Err(error))),
            task::Poll::Ready(Some(Ok(event))) => {
                match &event {
                    StackEvent::Resource {
                        resource_status,
                        details,
                    } => {
                        if resource_status.sentiment().is_negative() {
                            self.resource_error_events
                                .push((*resource_status, details.clone()));
                        }
                    }
                    StackEvent::Stack {
                        resource_status, ..
                    } => {
                        if resource_status.sentiment().is_negative() {
                            if let Some(reason) = event.resource_status_reason() {
                                self.stack_error_status_reason.replace(reason.to_string());
                            }
                        }
                        match (self.check_progress)(*resource_status) {
                            StackOperationStatus::InProgress | StackOperationStatus::Complete => {}
                            StackOperationStatus::Failed => {
                                self.stack_error_status = Some(*resource_status);
                            }
                            StackOperationStatus::Unexpected => {
                                panic!("stack has unexpected status for: {}", resource_status);
                            }
                        }
                    }
                }
                task::Poll::Ready(Some(Ok(event)))
            }
        }
    }
}
