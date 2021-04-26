//! An operation to 'apply' a CloudFormation template to an AWS environment.

use std::{fmt, future::Future, pin::Pin, task};

use async_stream::try_stream;
use chrono::{DateTime, Utc};
use futures_util::{Stream, TryFutureExt, TryStreamExt};
use rusoto_cloudformation::{
    CloudFormation, CreateChangeSetInput, DescribeStacksInput, Stack, Tag,
};
use rusoto_core::RusotoError;
use serde_plain::forward_display_to_serde;

use crate::change_set::{
    create_change_set, execute_change_set, ChangeSet, CreateChangeSetError, ExecuteChangeSetError,
};
use crate::{ChangeSetStatus, ResourceStatus, StackEvent, StackEventDetails, StackStatus, Status};

/// The input for the `apply` operation.
#[derive(Clone, Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct ApplyInput {
    /// Capabilities to explicitly acknowledge.
    ///
    /// See [`Capability`] for more information.
    pub capabilities: Vec<Capability>,

    /// A unique identifier for this `apply` operation.
    ///
    /// Specify this token if you plan to retry requests so that AWS CloudFormation knows that
    /// you're not attempting to apply a stack with the same name. You might retry `apply` requests
    /// to ensure that AWS CloudFormation successfully received them.
    ///
    /// All events triggered by a given stack operation are assigned the same client request token,
    /// which are used to track operations. If you do not specify a specific client request token,
    /// one will be generated in order to accurately correlate events with the performed stack
    /// operations.
    pub client_request_token: Option<String>,

    /// The Simple Notification Service (SNS) topic ARNs to publish stack related events.
    ///
    /// You can find your SNS topic ARNs using the SNS console or your Command Line Interface (CLI).
    pub notification_arns: Vec<String>,

    /// A list of input parameters for the stack.
    ///
    /// If you don't specify a key and value for a particular parameter, AWS CloudFormation uses the
    /// default value that is specified in your template.
    ///
    /// Note that, unlike when directly updating a stack, it is not possible to reuse previous
    /// values of parameters.
    pub parameters: Vec<Parameter>,

    /// The template resource types that you have permissions to work with for this `apply`
    /// operation, such as `AWS::EC2::Instance`, `AWS::EC2::*`, or `Custom::MyCustomInstance`.
    ///
    /// Use the following syntax to describe template resource types:
    ///
    /// - `AWS::*` for all AWS resources.
    /// - `Custom::*` for all custom resources.
    /// - `Custom::`*`logical_ID`* for a specific custom resource.
    /// - `AWS::`*`service_name`*`::*` for all resources of a particular AWS service.
    /// - `AWS::`*`service_name`*`::`*`resource_logical_ID`* for a specific AWS resource.
    ///
    /// If the list of resource types doesn't include a resource that you're applying, the operation
    /// fails. By default, AWS CloudFormation grants permissions to all resource types. AWS Identity
    /// and Access Management (IAM) uses this parameter for AWS CloudFormation-specific condition
    /// keys in IAM policies. For more information, see [Controlling Access with AWS Identity and
    /// Access Management][1].
    ///
    /// [1]: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/using-iam-template.html
    pub resource_types: Option<Vec<String>>,

    /// The Amazon Resource Name (ARN) of an AWS Identity And Access Management (IAM) role that AWS
    /// CloudFormation assumes to apply the stack.
    ///
    /// AWS CloudFormation uses the role's credentials to make calls on your behalf. AWS
    /// CloudFormation always uses this role for all future operations on the stack. As long as
    /// users have permission to operate on the stack, AWS CloudFormation uses this role even if the
    /// users don't have permission to pass it. Ensure that the role grants least privilege.
    ///
    /// If you don't specify a value, AWS CloudFormation uses the role that was previously
    /// associated with the stack. If no role is available, AWS CloudFormation uses a temporary
    /// session that is generated from your user credentials.
    pub role_arn: Option<String>,

    /// The name that is associated with the stack.
    ///
    /// The name must be unique in the region in which you are creating the stack.
    ///
    /// A stack name can contain only alphanumeric characters (case sensitive) and hyphens. It must
    /// start with an alphabetic character and cannot be longer than 128 characters.
    pub stack_name: String,

    /// Key-value pairs to associate with this stack.
    ///
    /// AWS CloudFormation also propagates these tags to the resources created in the stack. A
    /// maximum number of 50 tags can be specified.
    pub tags: Vec<Tag>,

    /// Source for the template body to apply.
    ///
    /// For more information about templates, go to [Template Anatomy][1] in the AWS CloudFormation
    /// User Guide.
    ///
    /// [1]: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-anatomy.html
    pub template_source: TemplateSource,
}

impl ApplyInput {
    fn into_raw(self) -> CreateChangeSetInput {
        let (template_body, template_url) = match self.template_source {
            TemplateSource::Inline { body } => (Some(body), None),
            TemplateSource::S3 { url } => (None, Some(url)),
        };
        CreateChangeSetInput {
            capabilities: Some(self.capabilities.iter().map(ToString::to_string).collect()),
            change_set_name: format!("apply-{}", Utc::now().timestamp_millis()),
            change_set_type: Some("CREATE".to_string()),
            notification_ar_ns: Some(self.notification_arns),
            parameters: Some(
                self.parameters
                    .into_iter()
                    .map(Parameter::into_raw)
                    .collect(),
            ),
            resource_types: self.resource_types,
            role_arn: self.role_arn,
            stack_name: self.stack_name,
            tags: Some(self.tags),
            template_body,
            template_url,
            ..CreateChangeSetInput::default()
        }
    }
}

/// In some cases, you must explicitly acknowledge that your stack template contains certain
/// capabilities in order for AWS CloudFormation to create (or update) the stack.
///
/// - `CAPABILITY_IAM` and `CAPABILITY_NAMED_IAM`
///
///   Some stack templates might include resources that can affect permissions in your AWS
///   account; for example, by creating new AWS Identity and Access Management (IAM) users. For
///   those stacks, you must explicitly acknowledge this by specifying one of these
///   capabilities.
///
///   The following IAM resources require you to specify either `CAPABILITY_IAM` or
///   `CAPABILITY_NAMES_IAM` capability.
///
///   - If you have IAM resources, you can specify either capability.
///   - If you have IAM resources with custom names, you *must* specify `CAPABILITY_NAMED_IAM`.
///   - If you don't specify either of these capabilities, AWS CloudFormation returns an
///    `InsufficientCapabilities` error.
///
///   If you stack template contains these resources, we recommend that you review all
///   permissions associated with them and edit their permissions if necessary.
///
///   - `AWS::IAM::AccessKey`
///   - `AWS::IAM::Group`
///   - `AWS::IAM::InstanceProfile`
///   - `AWS::IAM::Policy`
///   - `AWS::IAM::Role`
///   - `AWS::IAM::User`
///   - `AWS::IAM::UserToGroupAddition`
///
///   For more information, see
///   [Acknowledging IAM Resources in AWS CloudFormation Templates][1].
///
/// - `CAPABILITY_AUTO_EXPAND`
///
///   Some template contain macros. Macros perform custom processing on templates; this can
///   include simple actions like find-and-replace operations, all the way to extensive
///   transformations of entire templates. Because of this, users typically create a change set
///   from the processed template, so that they can review the changes resulting from the macros
///   before actually creating the stack. If your template contains one or more macros, and you
///   choose to create a stack directly from the processed template, without first reviewing the
///   resulting changes in a change set, you must acknowledge this capability. This includes the
///   [`AWS::Include`] and [`AWS::Serverless`] transforms, which are macros hosted by AWS
///   CloudFormation.
///
///   This capacity does not apply to creating change sets, and specifying it when creating
///   change sets has no effect.
///
///   If you want to create a stack from a stack template that contains macros *and* nested
///   stacks, you must create or update the stack directly from the template using the
///   `CreateStack` or `UpdateStack` action, and specifying this capability.
///
///   For more information on macros, see [Using AWS CloudFormation Macros to Perform Custom
///   Processing on Templates][2].
///
/// [1]: http://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/using-iam-template.html#capabilities
/// [2]: http://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-macros.html
/// [`AWS::Include`]: http://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/create-reusable-transform-function-snippets-and-add-to-your-template-with-aws-include-transform.html
/// [`AWS::Serverless`]: http://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/transform-aws-serverless.html
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub enum Capability {
    /// Acknowledge IAM resources (*without* custom names only).
    #[serde(rename = "CAPABILITY_IAM")]
    Iam,

    /// Acknowledge IAM resources (with or without custom names).
    #[serde(rename = "CAPABILITY_NAMED_IAM")]
    NamedIam,

    /// Acknowledge macro expansion.
    #[serde(rename = "CAPABILITY_AUTO_EXPAND")]
    AutoExpand,
}

forward_display_to_serde!(Capability);

/// An input parameter for an `apply` operation.
///
/// Note that, unlike when directly updating a stack, it is not possible to reuse previous
/// values of parameters.
#[derive(Clone, Debug)]
pub struct Parameter {
    /// The key associated with the parameter.
    pub key: String,

    /// The input value associated with the parameter.
    pub value: String,
}

impl Parameter {
    fn into_raw(self) -> rusoto_cloudformation::Parameter {
        rusoto_cloudformation::Parameter {
            parameter_key: Some(self.key),
            parameter_value: Some(self.value),
            ..rusoto_cloudformation::Parameter::default()
        }
    }
}

/// Source for a template body.
///
/// Templates can be specified for CloudFormation APIs in one of two ways:
///
/// - As a JSON string, inline with the request.
/// - As a URL to a template file on S3.
///
/// See the variant documentation for more information.
#[derive(Clone, Debug)]
pub enum TemplateSource {
    /// Structure containing the template body with a minimum length of 1 byte and a maximum length
    /// of 51,200 bytes.
    ///
    /// For more information, go to [Template Anatomy][1] in the AWS CloudFormation User Guide.
    ///
    /// [1]: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-anatomy.html
    Inline { body: String },

    /// Location of file containing the template body.
    ///
    /// The URL must point to a template (max size: 460,800 bytes) that is located in an Amazon S3
    /// bucket. For more information, go to the [Template Anatomy][1] in the AWS CloudFormation User
    /// Guide.
    ///
    /// [1]: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-anatomy.html
    S3 { url: String },
}

impl TemplateSource {
    /// Construct an [`Inline`](Self::Inline) template source for the given `body`.
    #[must_use]
    pub fn inline(body: impl Into<String>) -> Self {
        Self::Inline { body: body.into() }
    }

    /// Construct an [`S3`](Self::S3) template source for the given `url`.
    #[must_use]
    pub fn s3(url: impl Into<String>) -> Self {
        Self::S3 { url: url.into() }
    }
}

/// The output of the `apply` operation.
#[derive(Debug, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub struct ApplyOutput {
    /// The unique ID of the change set.
    pub change_set_id: String,

    /// The time at which the stack was created.
    pub creation_time: DateTime<Utc>,

    /// A user-defined description associated with the stack.
    pub description: Option<String>,

    /// The time the stack was last updated.
    ///
    /// This field will only be set if the stack has been updated at least once.
    pub last_updated_time: Option<DateTime<Utc>>,

    /// A list of output structures.
    pub outputs: Vec<StackOutput>,

    /// Unique identifier for the stack.
    pub stack_id: String,

    /// The name associated with the stack.
    pub stack_name: String,

    /// Current status of the stack.
    pub stack_status: StackStatus,

    /// A list of [`Tag`]s that specify information about the stack.
    pub tags: Vec<Tag>,
}

impl ApplyOutput {
    fn from_raw(stack: Stack) -> Self {
        Self {
            change_set_id: stack.change_set_id.expect("Stack without change_set_id"),
            creation_time: DateTime::parse_from_rfc3339(&stack.creation_time)
                .expect("Stack invalid creation_time")
                .into(),
            description: stack.description,
            last_updated_time: stack.last_updated_time.as_deref().map(|last_updated_time| {
                DateTime::parse_from_rfc3339(last_updated_time)
                    .expect("Stack invalid last_updated_time")
                    .into()
            }),
            outputs: stack
                .outputs
                .map(|outputs| {
                    outputs
                        .into_iter()
                        .map(|output| StackOutput {
                            description: output.description,
                            export_name: output.export_name,
                            key: output.output_key.expect("StackOutput without output_key"),
                            value: output
                                .output_value
                                .expect("StackOutput without output_value"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            stack_id: stack.stack_id.expect("Stack without stack_id"),
            stack_name: stack.stack_name,
            stack_status: stack.stack_status.parse().expect("invalid stack status"),
            tags: stack.tags.unwrap_or_default(),
        }
    }
}

/// An output from an `apply` operation.
#[derive(Debug, PartialEq)]
pub struct StackOutput {
    /// User defined description associated with the output.
    pub description: Option<String>,

    /// The name of the export associated with the output.
    pub export_name: Option<String>,

    /// The key associated with the output.
    pub key: String,

    /// The value associated with the output.
    pub value: String,
}

/// Events emitted by an `apply` operation.
#[derive(Debug, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub enum ApplyEvent {
    /// A stack event emitted by CloudFormation during the `apply` operation.
    Event(StackEvent),

    /// The output of the `apply` operation (meaning it has concluded successfully).
    Output(ApplyOutput),
}

/// Errors emitted by an `apply` operation.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub enum ApplyError {
    /// A CloudFormation API error occurred.
    ///
    /// This is likely to be due to invalid input parameters or missing CloudFormation permissions.
    /// The inner error should have a descriptive message.
    ///
    /// **Note:** the inner error will always be some variant of [`RusotoError`], but since they are
    /// generic over the type of service errors we either need a variant per API used, or `Box`. If
    /// you do need to programmatically match a particular API error you can use [`Box::downcast`].
    CloudFormationApi(Box<dyn std::error::Error>),

    /// The change set failed to create.
    ///
    /// Change sets are created asynchronously and may settle in a `FAILED` state. Trying to execute
    /// a `FAILED` change set will fail (who would have guessed). This error includes details of the
    /// failing change set to help diagnose errors.
    CreateChangeSetFailed {
        /// The id of the failed change set.
        id: String,

        /// The status of the failed change set.
        status: ChangeSetStatus,

        /// The reason the change set failed to create.
        status_reason: String,
    },

    /// The apply operation failed.
    ///
    /// This error tries to capture enough information to quickly identify the root-cause of the
    /// operation's failure (such as not having permission to create or update a particular resource
    /// in the stack).
    Failure {
        /// The ID of the stack.
        stack_id: String,

        /// The failed status in which the stack settled.
        stack_status: StackStatus,

        /// The *first* reason the stack moved into a failing state.
        ///
        /// Note that this is not the reason associated with the current `stack_status`, but rather
        /// the reason for the first negative status the stack entered (which is usually more
        /// descriptive).
        stack_status_reason: String,

        /// Resource events with negative statuses that may have precipitated the failure of the
        /// operation.
        ///
        /// **Note:** this is represented as a `Vec` or tuples to avoid having to worry about
        /// matching [`StackEvent`] variants (when it would be a logical error for them to be
        /// anything other than the `Resource` variant).
        resource_events: Vec<(ResourceStatus, StackEventDetails)>,
    },

    /// The apply operation succeeded with warnings.
    ///
    /// It is possible for resource errors to occur even when the overall operation succeeds, such
    /// as failing to delete a resource during clean-up after a successful update. Rather than
    /// letting this pass silently, or relying on carefully interrogating `StackEvent`s, the
    /// operation returns an error.
    ///
    /// Note that the error includes the [`ApplyOutput`], since the stack did settle into a
    /// successful status. If you don't care about non-critical resource errors you can use this to
    /// simply map this variant away:
    ///
    /// ```no_run
    /// # use cloudformatious::{ApplyError, ApplyOutput};
    /// # fn main() -> Result<(), ApplyError> {
    /// # let output = todo!();
    /// # let resource_events = Vec::new();
    /// let result = Err(ApplyError::Warning { output, resource_events });
    /// result.or_else(|error| {
    ///     if let ApplyError::Warning { output, .. } = error {
    ///         Ok(output)
    ///     } else {
    ///         Err(error)
    ///     }
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    Warning {
        /// The operation output.
        output: ApplyOutput,

        /// Resource events with negative statuses that did not affect the overall operation.
        ///
        /// **Note:** this is represented as a `Vec` or tuples to avoid having to worry about
        /// matching [`StackEvent`] variants (when it would be a logical error for them to be
        /// anything other than the `Resource` variant).
        resource_events: Vec<(ResourceStatus, StackEventDetails)>,
    },
}

impl ApplyError {
    fn from_rusoto_error<E: std::error::Error + 'static>(error: RusotoError<E>) -> Self {
        Self::CloudFormationApi(error.into())
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CloudFormationApi(error) => {
                write!(f, "CloudFormation API error: {}", error)
            }
            Self::CreateChangeSetFailed {
                id,
                status,
                status_reason,
            } => {
                write!(
                    f,
                    "Change set {} failed to create; terminal status: {} ({})",
                    id, status, status_reason
                )
            }
            Self::Failure {
                stack_id,
                stack_status,
                stack_status_reason,
                resource_events,
            } => {
                write!(
                    f,
                    "Stack {} failed to apply; terminal status: {} ({})",
                    stack_id, stack_status, stack_status_reason
                )?;

                if !resource_events.is_empty() {
                    writeln!(f, "\nThe following resources had errors:")?;
                }
                for (resource_status, details) in resource_events {
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
            Self::Warning {
                output,
                resource_events,
            } => {
                writeln!(
                    f,
                    "Stack {} applied successfully but some resources had errors:",
                    output.stack_id
                )?;
                for (resource_status, details) in resource_events {
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
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CloudFormationApi(error) => Some(error.as_ref()),
            Self::CreateChangeSetFailed { .. } | Self::Failure { .. } | Self::Warning { .. } => {
                None
            }
        }
    }
}

/// An ongoing `apply` operation.
///
/// This implements both `Future` and `Stream`, depending on whether or not it's desired to react to
/// stack progress or simply wait for the operation to conclude.
pub struct Apply<'client> {
    event_stream: Pin<Box<dyn Stream<Item = Result<ApplyEvent, ApplyError>> + 'client>>,

    // The `ApplyOutput` is moved here once it's been emitted by the stream.
    output: Option<ApplyOutput>,
}

impl Future for Apply<'_> {
    type Output = Result<ApplyOutput, ApplyError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Self::Output> {
        loop {
            match self.event_stream.as_mut().poll_next(ctx) {
                task::Poll::Pending => return task::Poll::Pending,
                task::Poll::Ready(Some(Err(error))) => return task::Poll::Ready(Err(error)),
                task::Poll::Ready(Some(Ok(ApplyEvent::Event(_)))) => continue,
                task::Poll::Ready(Some(Ok(ApplyEvent::Output(output)))) => {
                    self.output.replace(output);
                    continue;
                }
                task::Poll::Ready(None) => {
                    return task::Poll::Ready(Ok(self
                        .output
                        .take()
                        .expect("end of stream without output")))
                }
            }
        }
    }
}

impl Stream for Apply<'_> {
    type Item = Result<ApplyEvent, ApplyError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut task::Context,
    ) -> task::Poll<Option<Self::Item>> {
        self.event_stream.as_mut().poll_next(ctx)
    }
}

impl<'client> Apply<'client> {
    pub(crate) fn new<Client: CloudFormation>(
        client: &'client Client,
        mut input: ApplyInput,
    ) -> Self {
        let event_stream = try_stream! {
            input
                .client_request_token
                .get_or_insert_with(|| format!("apply-{}", Utc::now().timestamp_millis()));

            let change_set = match create_change_set_internal(client, input).await? {
                Ok(change_set) => change_set,
                Err(change_set) => {
                    let output = describe_output(client, change_set.stack_id).await?;
                    yield ApplyEvent::Output(output);
                    return;
                },
            };
            let stack_id = change_set.stack_id.clone();

            let mut execution = ExecuteChangeSet::new(client, stack_id, change_set);
            while let Some(event) = execution.try_next().await? {
                yield ApplyEvent::Event(event);
            }

            let output = execution.try_into_output().await?;
            yield ApplyEvent::Output(output);
        };
        Self {
            event_stream: Box::pin(event_stream),
            output: None,
        }
    }
}

async fn create_change_set_internal<Client: CloudFormation>(
    client: &Client,
    input: ApplyInput,
) -> Result<Result<ChangeSet, ChangeSet>, ApplyError> {
    let error = match create_change_set(client, input.into_raw()).await {
        Ok(change_set) => return Ok(Ok(change_set)),
        Err(error) => error,
    };
    match error {
        CreateChangeSetError::NoChanges(change_set) => Ok(Err(change_set)),
        CreateChangeSetError::CreateApi(error) => Err(ApplyError::from_rusoto_error(error)),
        CreateChangeSetError::PollApi(error) => Err(ApplyError::from_rusoto_error(error)),
        CreateChangeSetError::Failed(change_set) => Err(ApplyError::CreateChangeSetFailed {
            id: change_set.id,
            status: change_set.status,
            status_reason: change_set
                .status_reason
                .expect("ChangeSet failed without reason"),
        }),
    }
}

/// A stream that tracks useful error info.
struct ExecuteChangeSet<'client, Client> {
    client: &'client Client,
    stack_id: String,
    events: Pin<Box<dyn Stream<Item = Result<StackEvent, ExecuteChangeSetError>> + 'client>>,
    stack_error_status: Option<StackStatus>,
    stack_error_status_reason: Option<String>,
    resource_error_events: Vec<(ResourceStatus, StackEventDetails)>,
}

impl<'client, Client> ExecuteChangeSet<'client, Client>
where
    Client: CloudFormation,
{
    fn new(client: &'client Client, stack_id: String, change_set: ChangeSet) -> Self {
        ExecuteChangeSet {
            client,
            stack_id,
            events: Box::pin(execute_change_set(client, change_set)),
            stack_error_status: None,
            stack_error_status_reason: None,
            resource_error_events: Vec::new(),
        }
    }

    fn process_event(
        &mut self,
        event: Result<StackEvent, ExecuteChangeSetError>,
    ) -> Result<StackEvent, ApplyError> {
        let event = event.or_else(|error| match error {
            ExecuteChangeSetError::ExecuteApi(error) => Err(ApplyError::from_rusoto_error(error)),
            ExecuteChangeSetError::PollApi(error) => Err(ApplyError::from_rusoto_error(error)),
            ExecuteChangeSetError::Failed { status, event } => {
                self.stack_error_status = Some(status);
                Ok(event)
            }
        })?;

        match &event {
            StackEvent::Stack {
                resource_status, ..
            } => {
                if resource_status.sentiment().is_negative() {
                    if let Some(reason) = event.resource_status_reason() {
                        self.stack_error_status_reason.replace(reason.to_string());
                    }
                }
            }
            StackEvent::Resource {
                resource_status,
                details,
            } => {
                if resource_status.sentiment().is_negative() {
                    self.resource_error_events
                        .push((*resource_status, details.clone()));
                }
            }
        }

        Ok(event)
    }

    async fn try_into_output(self) -> Result<ApplyOutput, ApplyError> {
        if let Some(stack_status) = self.stack_error_status {
            return Err(ApplyError::Failure {
                stack_id: self.stack_id,
                stack_status,
                stack_status_reason: self
                    .stack_error_status_reason
                    .expect("stack op failed with no reasons"),
                resource_events: self.resource_error_events,
            });
        }

        let output = describe_output(self.client, self.stack_id).await?;

        if self.resource_error_events.is_empty() {
            Ok(output)
        } else {
            Err(ApplyError::Warning {
                output,
                resource_events: self.resource_error_events,
            })
        }
    }
}

impl<'client, Client> Stream for ExecuteChangeSet<'client, Client>
where
    Client: CloudFormation,
{
    type Item = Result<StackEvent, ApplyError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut task::Context<'_>,
    ) -> task::Poll<Option<Self::Item>> {
        match self.events.as_mut().poll_next(ctx) {
            task::Poll::Pending => task::Poll::Pending,
            task::Poll::Ready(None) => task::Poll::Ready(None),
            task::Poll::Ready(Some(event)) => {
                let event = self.process_event(event);
                task::Poll::Ready(Some(event))
            }
        }
    }
}

async fn describe_output<Client: CloudFormation>(
    client: &Client,
    stack_id: String,
) -> Result<ApplyOutput, ApplyError> {
    let describe_stacks_input = DescribeStacksInput {
        stack_name: Some(stack_id),
        ..DescribeStacksInput::default()
    };
    let stack = client
        .describe_stacks(describe_stacks_input)
        .map_err(ApplyError::from_rusoto_error)
        .await?
        .stacks
        .expect("DescribeStacksOutput without stacks")
        .pop()
        .expect("DescribeStacksOutput empty stacks");
    Ok(ApplyOutput::from_raw(stack))
}
