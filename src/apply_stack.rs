//! An operation to 'apply' a CloudFormation template to an AWS environment.

use std::{fmt, future::Future, pin::Pin, task};

use async_stream::try_stream;
use aws_sdk_cloudformation::{
    client::fluent_builders::CreateChangeSet, model::Stack, types::SdkError,
};
use aws_smithy_types_convert::date_time::DateTimeExt;
use chrono::{DateTime, Utc};
use futures_util::{Stream, TryFutureExt, TryStreamExt};

use crate::{
    change_set::{
        create_change_set, execute_change_set, ChangeSet, ChangeSetType, ChangeSetWithType,
        CreateChangeSetError, ExecuteChangeSetError,
    },
    stack::StackOperationError,
    BlockedStackStatus, ChangeSetStatus, StackEvent, StackFailure, StackStatus, StackWarning, Tag,
};

/// The input for the `apply_stack` operation.
///
/// You can create an apply stack input via the [`new`](Self::new) associated function. Setters are
/// also available to make construction as ergonomic as possible.
///
/// ```no_run
/// use cloudformatious::{ApplyStackInput, Capability, Parameter, Tag, TemplateSource};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let config = aws_config::load_from_env().await;
/// let client = cloudformatious::Client::new(&config);
/// let input = ApplyStackInput::new("my-stack", TemplateSource::inline("{}"))
///     .set_capabilities([Capability::Iam])
///     .set_client_request_token("hello")
///     .set_notification_arns(["arn:foo"])
///     .set_parameters([Parameter { key: "hello".to_string(), value: "world".to_string() }])
///     .set_resource_types(["AWS::IAM::Role"])
///     .set_role_arn("arn:foo")
///     .set_tags([Tag { key: "hello".to_string(), value: "world".to_string() }]);
/// let output = client.apply_stack(input).await?;
/// // ...
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct ApplyStackInput {
    /// Capabilities to explicitly acknowledge.
    ///
    /// See [`Capability`] for more information.
    pub capabilities: Vec<Capability>,

    /// A unique identifier for this `apply_stack` operation.
    ///
    /// Specify this token if you plan to retry requests so that AWS CloudFormation knows that
    /// you're not attempting to apply a stack with the same name. You might retry `apply_stack`
    /// requests to ensure that AWS CloudFormation successfully received them.
    ///
    /// All events triggered by a given stack operation are assigned the same client request token,
    /// which are used to track operations.
    pub client_request_token: Option<String>,

    /// Whether or not to disable rolling back in the event of a failure.
    ///
    /// When rollback is disabled, resources that were created/updated before the failing operation
    /// are preserved and the stack settles with a `*_FAILED` status. This may be helpful when
    /// debugging failing stack operations.
    pub disable_rollback: bool,

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

    /// The template resource types that you have permissions to work with for this `apply_stack`
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

impl ApplyStackInput {
    /// Construct an input for the given `stack_name` and `template_source`.
    pub fn new(stack_name: impl Into<String>, template_source: TemplateSource) -> Self {
        Self {
            stack_name: stack_name.into(),
            template_source,

            capabilities: Vec::new(),
            client_request_token: None,
            disable_rollback: false,
            notification_arns: Vec::new(),
            parameters: Vec::new(),
            resource_types: None,
            role_arn: None,
            tags: Vec::new(),
        }
    }

    /// Set the value for `capabilities`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_capabilities(mut self, capabilities: impl Into<Vec<Capability>>) -> Self {
        self.capabilities = capabilities.into();
        self
    }

    /// Set the value for `client_request_token`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_client_request_token(mut self, client_request_token: impl Into<String>) -> Self {
        self.client_request_token = Some(client_request_token.into());
        self
    }

    /// Set the value for `disable_rollback`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_disable_rollback(mut self, disable_rollback: bool) -> Self {
        self.disable_rollback = disable_rollback;
        self
    }

    /// Set the value for `notification_arns`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_notification_arns<I, S>(mut self, notification_arns: I) -> Self
    where
        I: Into<Vec<S>>,
        S: Into<String>,
    {
        self.notification_arns = notification_arns
            .into()
            .into_iter()
            .map(Into::into)
            .collect();
        self
    }

    /// Set the value for `parameters`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_parameters(mut self, parameters: impl Into<Vec<Parameter>>) -> Self {
        self.parameters = parameters.into();
        self
    }

    /// Set the value for `resource_types`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_resource_types<I, S>(mut self, resource_types: I) -> Self
    where
        I: Into<Vec<S>>,
        S: Into<String>,
    {
        self.resource_types = Some(resource_types.into().into_iter().map(Into::into).collect());
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

    /// Set the value for `tags`.
    ///
    /// **Note:** this consumes and returns `self` for chaining.
    #[must_use]
    pub fn set_tags(mut self, tags: impl Into<Vec<Tag>>) -> Self {
        self.tags = tags.into();
        self
    }

    fn configure(self, op: CreateChangeSet) -> (ChangeSetType, CreateChangeSet) {
        let change_set_type = ChangeSetType::Create;
        let (template_body, template_url) = match self.template_source {
            TemplateSource::Inline { body } => (Some(body), None),
            TemplateSource::S3 { url } => (None, Some(url)),
        };
        let input = op
            .set_capabilities(Some(
                self.capabilities
                    .into_iter()
                    .map(Capability::into_sdk)
                    .collect(),
            ))
            .change_set_name(format!("apply-stack-{}", Utc::now().timestamp_millis()))
            .change_set_type(change_set_type.into_sdk())
            .set_notification_ar_ns(Some(self.notification_arns))
            .set_parameters(Some(
                self.parameters
                    .into_iter()
                    .map(Parameter::into_sdk)
                    .collect(),
            ))
            .set_resource_types(self.resource_types)
            .set_role_arn(self.role_arn)
            .stack_name(self.stack_name)
            .set_tags(Some(self.tags.into_iter().map(Tag::into_sdk).collect()))
            .set_template_body(template_body)
            .set_template_url(template_url);

        (change_set_type, input)
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
pub enum Capability {
    /// Acknowledge IAM resources (*without* custom names only).
    #[display("CAPABILITY_IAM")]
    Iam,

    /// Acknowledge IAM resources (with or without custom names).
    #[display("CAPABILITY_NAMED_IAM")]
    NamedIam,

    /// Acknowledge macro expansion.
    #[display("CAPABILITY_AUTO_EXPAND")]
    AutoExpand,
}

impl Capability {
    fn into_sdk(self) -> aws_sdk_cloudformation::model::Capability {
        match self {
            Self::Iam => aws_sdk_cloudformation::model::Capability::CapabilityIam,
            Self::NamedIam => aws_sdk_cloudformation::model::Capability::CapabilityNamedIam,
            Self::AutoExpand => aws_sdk_cloudformation::model::Capability::CapabilityAutoExpand,
        }
    }
}

/// An input parameter for an `apply_stack` operation.
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
    fn into_sdk(self) -> aws_sdk_cloudformation::model::Parameter {
        aws_sdk_cloudformation::model::Parameter::builder()
            .parameter_key(self.key)
            .parameter_value(self.value)
            .build()
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

/// The output of the `apply_stack` operation.
#[derive(Debug, Eq, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub struct ApplyStackOutput {
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

impl ApplyStackOutput {
    fn from_raw(stack: Stack) -> Self {
        Self {
            change_set_id: stack.change_set_id.expect("Stack without change_set_id"),
            creation_time: stack
                .creation_time
                .expect("Stack without creation_time")
                .to_chrono_utc(),
            description: stack.description,
            last_updated_time: stack
                .last_updated_time
                .as_ref()
                .map(DateTimeExt::to_chrono_utc),
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
            stack_name: stack.stack_name.expect("Stack without stack_name"),
            stack_status: stack
                .stack_status
                .expect("Stack without stack_status")
                .as_str()
                .parse()
                .expect("invalid stack status"),
            tags: stack
                .tags
                .unwrap_or_default()
                .into_iter()
                .map(Tag::from_sdk)
                .collect(),
        }
    }
}

/// An output from an `apply_stack` operation.
#[derive(Debug, Eq, PartialEq)]
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

/// Errors emitted by an `apply_stack` operation.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub enum ApplyStackError {
    /// A CloudFormation API error occurred.
    ///
    /// This is likely to be due to invalid input parameters or missing CloudFormation permissions.
    /// The inner error should have a descriptive message.
    ///
    /// **Note:** the inner error will always be some variant of [`SdkError`], but since they are
    /// generic over the type of service errors we either need a variant per API used, or `Box`. If
    /// you do need to programmatically match a particular API error you can use [`Box::downcast`].
    CloudFormationApi(Box<dyn std::error::Error>),

    /// The stack cannot be modified as it's in a blocked state.
    Blocked {
        /// The blocked status that the stack is in.
        status: BlockedStackStatus,
    },

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

    /// The apply stack operation failed.
    Failure(StackFailure),

    /// The apply stack operation succeeded with warnings.
    ///
    /// It is possible for resource errors to occur even when the overall operation succeeds, such
    /// as failing to delete a resource during clean-up after a successful update. Rather than
    /// letting this pass silently, or relying on carefully interrogating `StackEvent`s, the
    /// operation returns an error.
    ///
    /// Note that the error includes the [`ApplyStackOutput`], since the stack did settle into a
    /// successful status. If you don't care about non-critical resource errors you can use this to
    /// simply map this variant away:
    ///
    /// ```no_run
    /// # use cloudformatious::ApplyStackError;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), ApplyStackError> {
    /// # let client: cloudformatious::Client = todo!();
    /// # let input = todo!();
    /// let output = client
    ///     .apply_stack(input)
    ///     .await
    ///     .or_else(|error| match error {
    ///         ApplyStackError::Warning { output, .. } => Ok(output),
    ///         error => Err(error),
    ///     })?;
    /// # Ok(())
    /// # }
    /// ```
    Warning {
        /// The operation output.
        output: ApplyStackOutput,

        /// Details of what went wrong.
        warning: StackWarning,
    },
}

impl ApplyStackError {
    fn from_sdk_error<E: std::error::Error + 'static>(error: SdkError<E>) -> Self {
        Self::CloudFormationApi(error.into())
    }
}

impl fmt::Display for ApplyStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CloudFormationApi(error) => {
                write!(f, "CloudFormation API error: {}", error)
            }
            Self::Blocked { status } => {
                write!(
                    f,
                    "stack operation failed because the stack is in a blocked state: {}",
                    status
                )
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
            Self::Failure(failure) => write!(f, "{}", failure),
            Self::Warning { warning, .. } => write!(f, "{}", warning),
        }
    }
}

impl std::error::Error for ApplyStackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CloudFormationApi(error) => Some(error.as_ref()),
            Self::Blocked { .. }
            | Self::CreateChangeSetFailed { .. }
            | Self::Failure { .. }
            | Self::Warning { .. } => None,
        }
    }
}

/// An ongoing `apply_stack` operation.
///
/// This implements `Future`, which will simply wait for the operation to conclude. If you want to
/// observe progress, see [`ApplyStack::events`].
pub struct ApplyStack<'client> {
    /// The stream of internal events that drives the different levels of the API.
    ///
    /// This might not be the best way of driving things, but it works with a few rough edges in the
    /// form of various possible panics that could arise from unanticipated execution patterns (e.g.
    /// attempting to await multiple times, or calling APIs out of order).
    event_stream: Pin<Box<dyn Stream<Item = Result<ApplyStackEvent, ApplyStackError>> + 'client>>,

    /// The `ApplyStackOutput` is moved here once it's been emitted by the stream.
    output: Option<Result<ApplyStackOutput, ApplyStackError>>,
}

impl<'client> ApplyStack<'client> {
    pub(crate) fn new(
        client: &'client aws_sdk_cloudformation::Client,
        input: ApplyStackInput,
    ) -> Self {
        let disable_rollback = input.disable_rollback;

        let event_stream = try_stream! {
            let (stack_id, change_set_id, change_set_type) =
                match create_change_set_internal(client, input).await? {
                    Ok(ChangeSetWithType {
                        change_set,
                        change_set_type,
                    }) => {
                        let stack_id = change_set.stack_id.clone();
                        let change_set_id = change_set.change_set_id.clone();
                        yield ApplyStackEvent::ChangeSet(change_set);
                        (stack_id, change_set_id, change_set_type)
                    }
                    Err(ChangeSetWithType { change_set, .. }) => {
                        let stack_id = change_set.stack_id.clone();
                        yield ApplyStackEvent::ChangeSet(change_set);

                        let output = describe_output(client, stack_id).await?;
                        yield ApplyStackEvent::Output(output);
                        return;
                    }
                };

            let mut operation =
                execute_change_set(client, stack_id.clone(), change_set_id, change_set_type, disable_rollback)
                    .await
                    .map_err(|error| match error {
                        ExecuteChangeSetError::ExecuteApi(error) => ApplyStackError::from_sdk_error(error),
                        ExecuteChangeSetError::Blocked { status } => ApplyStackError::Blocked { status },
                    })?;
            while let Some(event) = operation
                .try_next()
                .await
                .map_err(ApplyStackError::from_sdk_error)?
            {
                yield ApplyStackEvent::Event(event);
            }

            let warning = match operation.verify() {
                Err(StackOperationError::Failure(failure)) => {
                    Err(ApplyStackError::Failure(failure))?;
                    unreachable!()
                }
                Ok(_) => None,
                Err(StackOperationError::Warning(warning)) => Some(warning),
            };

            let output = describe_output(client, stack_id).await?;

            match warning {
                Some(warning) => {
                    Err(ApplyStackError::Warning { output, warning })?;
                    unreachable!()
                }
                None => yield ApplyStackEvent::Output(output),
            };
        };
        Self {
            event_stream: Box::pin(event_stream),
            output: None,
        }
    }

    /// Get the `ChangeSet` that will be applied.
    ///
    /// The change set will not be executed if you never poll again.
    pub fn change_set(&mut self) -> ApplyStackChangeSet<'client, '_> {
        ApplyStackChangeSet(self)
    }

    /// Get a `Stream` of `StackEvent`s.
    pub fn events(&mut self) -> ApplyStackEvents<'client, '_> {
        ApplyStackEvents(self)
    }
}

impl Future for ApplyStack<'_> {
    type Output = Result<ApplyStackOutput, ApplyStackError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Self::Output> {
        loop {
            match self.event_stream.as_mut().poll_next(ctx) {
                task::Poll::Pending => return task::Poll::Pending,
                task::Poll::Ready(None) => {
                    return task::Poll::Ready(
                        self.output
                            .take()
                            .expect("end of stream without err or output"),
                    )
                }
                task::Poll::Ready(Some(Ok(
                    ApplyStackEvent::ChangeSet(_) | ApplyStackEvent::Event(_),
                ))) => continue,
                task::Poll::Ready(Some(Ok(ApplyStackEvent::Output(output)))) => {
                    self.output.replace(Ok(output));
                    continue;
                }
                task::Poll::Ready(Some(Err(error))) => {
                    self.output.replace(Err(error));
                    continue;
                }
            }
        }
    }
}

/// Return value of [`ApplyStack::change_set`].
#[allow(clippy::module_name_repetitions)]
pub struct ApplyStackChangeSet<'client, 'apply>(&'apply mut ApplyStack<'client>);

impl Future for ApplyStackChangeSet<'_, '_> {
    type Output = Result<ChangeSet, ApplyStackError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut task::Context) -> task::Poll<Self::Output> {
        loop {
            match self.0.event_stream.as_mut().poll_next(ctx) {
                task::Poll::Pending => return task::Poll::Pending,
                task::Poll::Ready(None) => match self.0.output.take() {
                    None => panic!("end of stream without change set"),
                    Some(Ok(_)) => panic!("saw output before change set"),
                    Some(Err(error)) => return task::Poll::Ready(Err(error)),
                },
                task::Poll::Ready(Some(Ok(ApplyStackEvent::ChangeSet(change_set)))) => {
                    return task::Poll::Ready(Ok(change_set));
                }
                task::Poll::Ready(Some(Ok(ApplyStackEvent::Event(_)))) => {
                    panic!("saw stack event before change set");
                }
                task::Poll::Ready(Some(Ok(ApplyStackEvent::Output(_)))) => {
                    panic!("saw output before change set");
                }
                task::Poll::Ready(Some(Err(error))) => {
                    self.0.output.replace(Err(error));
                    continue;
                }
            }
        }
    }
}

/// Return value of [`ApplyStack::events`].
#[allow(clippy::module_name_repetitions)]
pub struct ApplyStackEvents<'client, 'apply>(&'apply mut ApplyStack<'client>);

impl Stream for ApplyStackEvents<'_, '_> {
    type Item = StackEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut task::Context,
    ) -> task::Poll<Option<Self::Item>> {
        loop {
            match self.0.event_stream.as_mut().poll_next(ctx) {
                task::Poll::Pending => return task::Poll::Pending,
                task::Poll::Ready(None) => return task::Poll::Ready(None),
                task::Poll::Ready(Some(Ok(ApplyStackEvent::ChangeSet(_)))) => continue,
                task::Poll::Ready(Some(Ok(ApplyStackEvent::Event(event)))) => {
                    return task::Poll::Ready(Some(event))
                }
                task::Poll::Ready(Some(Ok(ApplyStackEvent::Output(output)))) => {
                    self.0.output.replace(Ok(output));
                    return task::Poll::Ready(None);
                }
                task::Poll::Ready(Some(Err(error))) => {
                    self.0.output.replace(Err(error));
                    return task::Poll::Ready(None);
                }
            }
        }
    }
}

/// Events emitted by an `apply_stack` operation internally.
enum ApplyStackEvent {
    /// The change set has been created.
    ChangeSet(ChangeSet),

    /// A stack event emitted by CloudFormation during the `apply_stack` operation.
    Event(StackEvent),

    /// The output of the `apply_stack` operation (meaning it has concluded successfully).
    Output(ApplyStackOutput),
}

async fn create_change_set_internal(
    client: &aws_sdk_cloudformation::Client,
    input: ApplyStackInput,
) -> Result<Result<ChangeSetWithType, ChangeSetWithType>, ApplyStackError> {
    let (change_set_type, input) = input.configure(client.create_change_set());
    let error = match create_change_set(client, change_set_type, input).await {
        Ok(change_set) => return Ok(Ok(change_set)),
        Err(error) => error,
    };
    match error {
        CreateChangeSetError::NoChanges(change_set) => Ok(Err(change_set)),
        CreateChangeSetError::CreateApi(error) => Err(ApplyStackError::from_sdk_error(error)),
        CreateChangeSetError::PollApi(error) => Err(ApplyStackError::from_sdk_error(error)),
        CreateChangeSetError::Blocked { status } => Err(ApplyStackError::Blocked { status }),
        CreateChangeSetError::Failed(ChangeSetWithType { change_set, .. }) => {
            Err(ApplyStackError::CreateChangeSetFailed {
                id: change_set.change_set_id,
                status: change_set.status,
                status_reason: change_set
                    .status_reason
                    .expect("ChangeSet failed without reason"),
            })
        }
    }
}

async fn describe_output(
    client: &aws_sdk_cloudformation::Client,
    stack_id: String,
) -> Result<ApplyStackOutput, ApplyStackError> {
    let stack = client
        .describe_stacks()
        .stack_name(stack_id)
        .send()
        .map_err(ApplyStackError::from_sdk_error)
        .await?
        .stacks
        .expect("DescribeStacksOutput without stacks")
        .pop()
        .expect("DescribeStacksOutput empty stacks");
    Ok(ApplyStackOutput::from_raw(stack))
}

#[cfg(test)]
mod tests {
    use super::Capability;

    #[test]
    fn test_parse_display() {
        assert_eq!(Capability::Iam.to_string(), "CAPABILITY_IAM");
        assert_eq!(Capability::Iam, "CAPABILITY_IAM".parse().unwrap());
        assert_eq!(Capability::NamedIam.to_string(), "CAPABILITY_NAMED_IAM");
        assert_eq!(
            Capability::NamedIam,
            "CAPABILITY_NAMED_IAM".parse().unwrap(),
        );
        assert_eq!(Capability::AutoExpand.to_string(), "CAPABILITY_AUTO_EXPAND");
        assert_eq!(
            Capability::AutoExpand,
            "CAPABILITY_AUTO_EXPAND".parse().unwrap(),
        );
    }
}
