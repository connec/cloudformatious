use std::{collections::BTreeMap, fmt, iter, pin::Pin, task, time::Duration};

use async_stream::try_stream;
use aws_sdk_cloudformation::{
    error::SdkError, operation::describe_stack_events::DescribeStackEventsError,
};
use aws_smithy_types_convert::date_time::DateTimeExt;
use chrono::{DateTime, Utc};
use futures_util::{stream, Stream, TryStreamExt};

use crate::{
    status_reason::StatusReason, ResourceStatus, StackEvent, StackEventDetails, StackStatus, Status,
};

const POLL_INTERVAL_STACK_EVENT: Duration = Duration::from_secs(5);

/// Describes a failed stack operation.
///
/// This error tries to capture enough information to quickly identify the root-cause of the
/// operation's failure (such as not having permission to create or update a particular resource
/// in the stack). [`stack_status_reason`](Self::stack_status_reason) and
/// [`StackEventDetails::resource_status_reason`] may be useful for this purpose.
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

impl StackFailure {
    /// The *first* reason the stack moved into a failing state.
    #[must_use]
    pub fn stack_status_reason(&self) -> StatusReason {
        StatusReason::new(Some(&self.stack_status_reason))
    }
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
        Box<dyn Stream<Item = Result<StackEvent, SdkError<DescribeStackEventsError>>> + 'client>,
    >,
    stack_error_status: Option<StackStatus>,
    stack_error_status_reason: Option<String>,
    resource_error_events: Vec<(ResourceStatus, StackEventDetails)>,
}

impl<'client, F> StackOperation<'client, F>
where
    F: Fn(StackStatus) -> StackOperationStatus + Unpin,
{
    pub(crate) fn new(
        client: &'client aws_sdk_cloudformation::Client,
        stack_id: String,
        started_at: DateTime<Utc>,
        check_progress: F,
    ) -> Self {
        let root_stack_id = stack_id.clone();
        let events = try_stream! {
            let mut interval = tokio::time::interval(POLL_INTERVAL_STACK_EVENT);
            let mut since = started_at;
            let mut nested_stacks = BTreeMap::<String, String>::new();

            loop {
                interval.tick().await;

                let stack_ids = iter::once(root_stack_id.clone()).chain(nested_stacks.keys().cloned());
                let stack_events = stack_ids.into_iter().map(|stack_id| stream::once(Box::pin(async {
                    let stack_events: Vec<_> = client
                        .describe_stack_events()
                        .stack_name(stack_id)
                        .send()
                        .await?
                        .stack_events
                        .expect("DescribeStackEventsOutput without stack_events")
                        .into_iter()
                        .take_while(|event| {
                            event
                                .timestamp
                                .expect("StackEvent without timestamp")
                                .to_chrono_utc()
                                .expect("invalid timestamp")
                                > since
                        })
                        .map(|event| {
                            let stack_alias = event.stack_id().and_then(|stack_id| nested_stacks.get(stack_id)).cloned();
                            StackEvent::from_sdk(stack_alias, event)
                        })
                        .filter(|event| {
                            match event {
                                StackEvent::Stack { details, .. } => details.stack_id() == root_stack_id,
                                StackEvent::Resource{ .. } => true,
                            }
                        })
                        .collect();
                    Ok::<_, SdkError<DescribeStackEventsError>>(stack_events)
                })));
                let stack_events: Vec<_> = stream::select_all(stack_events).try_collect().await?;
                let stack_events: Vec<_> = stack_events.into_iter().flatten().collect();

                if let Some(stack_event) = stack_events.first() {
                    since = *stack_event.timestamp();
                }

                for stack_event in stack_events.into_iter().rev() {
                    let is_terminal = stack_event.is_terminal();

                    match &stack_event {
                        StackEvent::Resource {
                            details: details @ StackEventDetails {
                                physical_resource_id: Some(nested_stack_id),
                                ..
                            },
                            ..
                        } if details.resource_type() == "AWS::CloudFormation::Stack" && !nested_stack_id.is_empty() => {
                            let stack_alias = nested_stacks
                                .get(details.stack_id())
                                .map(String::as_str)
                                .into_iter()
                                .chain(iter::once(details.logical_resource_id()))
                                .collect::<Vec<_>>()
                                .join("/");
                            nested_stacks.insert(nested_stack_id.clone(), stack_alias);
                        },
                        _ => {},
                    }

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

    pub(crate) fn verify(self) -> Result<(), StackOperationError> {
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
    type Item = Result<StackEvent, SdkError<DescribeStackEventsError>>;

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
                    } if event.stack_id() == self.stack_id => {
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
                    // Do nothing for nested stack events since resource events from the parent stack will be processed.
                    StackEvent::Stack { .. } => {}
                }
                task::Poll::Ready(Some(Ok(event)))
            }
        }
    }
}
