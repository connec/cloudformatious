//! Helpers for working with change sets.

use std::{convert::TryFrom, fmt, time::Duration};

use aws_sdk_cloudformation::{
    error::{ProvideErrorMetadata, SdkError},
    operation::create_change_set::builders::CreateChangeSetFluentBuilder,
    operation::describe_change_set::{DescribeChangeSetError, DescribeChangeSetOutput},
    types::{Change, ChangeAction},
};
use aws_smithy_types_convert::date_time::DateTimeExt;
use chrono::{DateTime, Utc};
use enumset::EnumSet;
use futures_util::TryFutureExt;
use regex::Regex;
use tokio::time::{interval_at, Instant};

use crate::{
    stack::{StackOperation, StackOperationStatus},
    BlockedStackStatus, Capability, ChangeSetStatus, StackStatus, Tag,
};

const POLL_INTERVAL_CHANGE_SET: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeSetType {
    Create,
    Update,
}

impl ChangeSetType {
    pub(crate) fn into_sdk(self) -> aws_sdk_cloudformation::types::ChangeSetType {
        match self {
            ChangeSetType::Create => aws_sdk_cloudformation::types::ChangeSetType::Create,
            ChangeSetType::Update => aws_sdk_cloudformation::types::ChangeSetType::Update,
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

/// A planned set of changes to apply to a CloudFormation stack.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub struct ChangeSet {
    /// Capabilities that were explicitly acknowledged when the change set was created.
    ///
    /// See [`Capability`] for more information.
    pub capabilities: Vec<Capability>,

    /// The ARN of the change set.
    pub change_set_id: String,

    /// The name of the change set.
    pub change_set_name: String,

    /// A list of structures that describe the resources AWS CloudFormation changes if you execute
    /// the change set.
    pub changes: Vec<ResourceChange>,

    /// The start time when the change set was created.
    pub creation_time: DateTime<Utc>,

    /// Information about the change set.
    pub description: Option<String>,

    /// The change set's execution status.
    ///
    /// If the change set execution status is [`Available`], you can execute the change set. If you
    /// can’t execute the change set, the [`status`] indicates why. For example, a change set might
    /// be in an [`Unavailable`] state because AWS CloudFormation is still creating it or in an
    /// [`Obsolete`] state because the stack was already updated.
    ///
    /// [`Available`]: ExecutionStatus::Available
    /// [`Obsolete`]: ExecutionStatus::Obsolete
    /// [`Unavailable`]: ExecutionStatus::Unavailable
    /// [`status`]: Self::status
    pub execution_status: ExecutionStatus,

    /// The Simple Notification Service (SNS) topic ARNs to publish stack related events.
    pub notification_arns: Vec<String>,

    /// A list of structures that describes the input parameters and their values used to create the
    /// change set.
    pub parameters: Vec<Parameter>,

    /// The ARN of the stack that is associated with the change set.
    pub stack_id: String,

    /// The name of the stack that is associated with the change set.
    pub stack_name: String,

    /// The current status of the change set.
    pub status: ChangeSetStatus,

    /// A description of the change set's status.
    ///
    /// For example, if your attempt to create a change set failed, AWS CloudFormation shows the
    /// error message.
    pub status_reason: Option<String>,

    /// If you execute the change set, the tags that will be associated with the stack.
    pub tags: Vec<Tag>,
}

impl ChangeSet {
    fn from_sdk(change_set: DescribeChangeSetOutput) -> Self {
        Self {
            capabilities: change_set
                .capabilities
                .unwrap_or_default()
                .into_iter()
                .map(|capability| {
                    capability
                        .as_str()
                        .parse()
                        .expect("DescribeChangeSetOutput with invalid Capability")
                })
                .collect(),
            change_set_id: change_set
                .change_set_id
                .expect("DescribeChangeSetOutput without change_set_id"),
            change_set_name: change_set
                .change_set_name
                .expect("DescribeChangeSetOutput without change_set_name"),
            changes: change_set
                .changes
                .unwrap_or_default()
                .into_iter()
                .map(ResourceChange::from_sdk)
                .collect(),
            creation_time: change_set
                .creation_time
                .expect("DescribeChangeSetOutput without creation_time")
                .to_chrono_utc()
                .expect("invalid creation_time"),
            description: change_set.description,
            execution_status: change_set
                .execution_status
                .expect("DescribeChangeSetOutput without execution_status")
                .as_str()
                .parse()
                .expect("DescribeChangeSetOutput with invalid execution_status"),
            notification_arns: change_set.notification_ar_ns.unwrap_or_default(),
            parameters: change_set
                .parameters
                .unwrap_or_default()
                .into_iter()
                .map(Parameter::from_sdk)
                .collect(),
            stack_id: change_set
                .stack_id
                .expect("DescribeChangeSetOutput without stack_id"),
            stack_name: change_set
                .stack_name
                .expect("DescribeChangeSetOutput without stack_name"),
            status: change_set
                .status
                .expect("DescribeChangeSetOutput without status")
                .as_str()
                .parse()
                .expect("DescribeChangeSetOutput unexpected status"),
            status_reason: change_set.status_reason,
            tags: change_set
                .tags
                .unwrap_or_default()
                .into_iter()
                .map(Tag::from_sdk)
                .collect(),
        }
    }
}

/// The change set's execution status.
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
#[display(style = "SNAKE_CASE")]
pub enum ExecutionStatus {
    /// The change set is not available to execute.
    Unavailable,

    /// The change set is available to execute.
    Available,

    /// The change set is executing.
    ExecuteInProgress,

    /// The change set has been executed successfully.
    ExecuteComplete,

    /// The change set execution failed.
    ExecuteFailed,

    /// The stack was updated by another means after this change set was created.
    Obsolete,
}

/// A parameter set for a change set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parameter {
    /// The key associated with the parameter.
    pub parameter_key: String,

    /// The input value associated with the parameter.
    ///
    /// If you don't specify a key and value for a particular parameter, CloudFormation uses the
    /// default or previous value that's specified in your template.
    pub parameter_value: Option<String>,

    /// The existing parameter value will be used on update.
    pub use_previous_value: Option<bool>,

    /// The value that corresponds to a SSM parameter key.
    ///
    /// This field is returned only for [`SSM`] parameter types in the template.
    ///
    /// [`SSM`]: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/parameters-section-structure.html#aws-ssm-parameter-types
    pub resolved_value: Option<String>,
}

impl Parameter {
    fn from_sdk(param: aws_sdk_cloudformation::types::Parameter) -> Self {
        Self {
            parameter_key: param
                .parameter_key
                .expect("Parameter without parameter_key"),
            parameter_value: param.parameter_value,
            use_previous_value: param.use_previous_value,
            resolved_value: param.resolved_value,
        }
    }
}

/// The resource and the action that AWS CloudFormation will perform on it if you execute this
/// change set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceChange {
    /// The action that AWS CloudFormation takes on the resource.
    pub action: Action,

    /// The resource's logical ID, which is defined in the stack's template.
    pub logical_resource_id: String,

    /// The resource's physical ID (resource name).
    ///
    /// Resources that you are adding don't have physical IDs because they haven't been created.
    pub physical_resource_id: Option<String>,

    /// The type of AWS CloudFormation resource.
    pub resource_type: String,
}

impl ResourceChange {
    fn from_sdk(change: Change) -> Self {
        assert!(
            matches!(
                change.r#type,
                Some(aws_sdk_cloudformation::types::ChangeType::Resource)
            ),
            "Change with unexpected type {:?}",
            change.r#type
        );
        let change = change
            .resource_change
            .expect("Change without resource_change");
        let resource_type = change
            .resource_type
            .expect("ResourceChange without resource_type");
        Self {
            action: Action::from_sdk(
                &resource_type,
                &change.action.expect("ResourceChange without action"),
                change.details,
                change.replacement,
                change.scope,
            ),
            logical_resource_id: change
                .logical_resource_id
                .expect("ResourceChange without logical_resource_id"),
            physical_resource_id: change.physical_resource_id,
            resource_type,
        }
    }
}

/// The action that AWS CloudFormation takes on a resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Adds a new resource.
    Add,

    /// Changes a resource.
    Modify(ModifyDetail),

    /// Deletes a resource.
    Remove,

    /// Imports a resource.
    Import,

    /// Exact action for the resource cannot be determined.
    Dynamic,
}

impl Action {
    fn from_sdk(
        resource_type: &str,
        action: &ChangeAction,
        details: Option<Vec<aws_sdk_cloudformation::types::ResourceChangeDetail>>,
        replacement: Option<aws_sdk_cloudformation::types::Replacement>,
        scope: Option<Vec<aws_sdk_cloudformation::types::ResourceAttribute>>,
    ) -> Self {
        match action {
            ChangeAction::Add
            | ChangeAction::Remove
            | ChangeAction::Import
            | ChangeAction::Dynamic => {
                assert!(
                    matches!(details.as_deref(), None | Some([])),
                    "ResourceChange with action {:?} and details",
                    action
                );
                assert!(
                    replacement.is_none(),
                    "ResourceChange with action {:?} and replacement",
                    action
                );
                assert!(
                    scope.unwrap_or_default().is_empty(),
                    "ResourceChange with action {:?} and scope",
                    action
                );
                match action {
                    ChangeAction::Add => Self::Add,
                    ChangeAction::Remove => Self::Remove,
                    ChangeAction::Import => Self::Import,
                    ChangeAction::Dynamic => Self::Dynamic,
                    _ => unreachable!(),
                }
            }
            ChangeAction::Modify => Self::Modify(ModifyDetail::from_sdk(
                resource_type,
                details.expect("ResourceChange with action \"Modify\" without details"),
                &replacement.expect("ResourceChange with action \"Modify\" without replacement"),
                scope.expect("ResourceChange with action \"Modify\" without scope"),
            )),
            _ => panic!("ResourceChange with invalid action {:?}", action),
        }
    }
}

/// Additional detail for resource modifications.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModifyDetail {
    /// A list of structures that describe the changes that AWS CloudFormation will make to the
    /// resource.
    pub details: Vec<ResourceChangeDetail>,

    /// Indicates whether AWS CloudFormation will replace the resource by creating a new one and
    /// deleting the old one.
    pub replacement: Replacement,

    /// Indicates which resource attribute is triggering this update.
    pub scope: EnumSet<ModifyScope>,
}

impl ModifyDetail {
    fn from_sdk(
        resource_type: &str,
        details: Vec<aws_sdk_cloudformation::types::ResourceChangeDetail>,
        replacement: &aws_sdk_cloudformation::types::Replacement,
        scope: Vec<aws_sdk_cloudformation::types::ResourceAttribute>,
    ) -> Self {
        Self {
            details: details
                .into_iter()
                .map(|detail| ResourceChangeDetail::from_sdk(resource_type, detail))
                .collect(),
            replacement: replacement
                .as_str()
                .parse()
                .expect("ResourceChange with invalid replacement"),
            scope: scope
                .into_iter()
                .map(|scope| {
                    scope
                        .as_str()
                        .parse::<ModifyScope>()
                        .expect("ResourceChange with invalid scope")
                })
                .collect(),
        }
    }
}

/// Indicates whether AWS CloudFormation will replace the resource by creating a new one and
/// deleting the old one.
///
/// This value depends on the value of the `requires_recreation` property in the
/// [`ResourceTargetDefinition`] structure. For example, if the `requires_recreation` field is
/// [`Always`] and the `evaluation` field is [`Static`], `Replacement` is `True`. If the
/// `requires_recreation` field is `Always` and the `evaluation` field is [`Dynamic`],
/// `Replacement` is `Conditionally`.
///
/// If you have multiple changes with different `requires_recreation` values, the `Replacement`
/// value depends on the change with the most impact. A `requires_recreation` value of `Always` has
/// the most impact, followed by [`Conditionally`], and then [`Never`].
///
/// [`Always`]: RequiresRecreation::Always
/// [`Conditionally`]: RequiresRecreation::Conditionally
/// [`Never`]: RequiresRecreation::Never
/// [`Static`]: Evaluation::Static
/// [`Dynamic`]: Evaluation::Dynamic
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
pub enum Replacement {
    /// The resource will be replaced.
    True,

    /// The resource will not be replaced.
    False,

    /// The resource *may* be replaced.
    Conditional,
}

// The derive for EnumSetType creates an item that triggers this lint, so it has to be disabled
// at the module level. We don't want to disable it too broadly though, so we wrap its declaration
// in a module and re-export from that.
mod modify_scope {
    #![allow(clippy::expl_impl_clone_on_copy)]

    /// Indicates which resource attribute is triggering this update.
    #[derive(Debug, enumset::EnumSetType, parse_display::Display, parse_display::FromStr)]
    #[enumset(no_ops)]
    pub enum ModifyScope {
        /// A change to the resource's properties.
        Properties,

        /// A change to the resource's metadata.
        Metadata,

        /// A change to the resource's creation policy.
        CreationPolicy,

        /// A change to the resource's update policy.
        UpdatePolicy,

        /// A change to the resource's deletion policy.
        DeletionPolicy,

        /// A change to the resource's tags.
        Tags,
    }
}
pub use modify_scope::ModifyScope;

/// A change that AWS CloudFormation will make to a resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceChangeDetail {
    /// The group to which the CausingEntity value belongs.
    ///
    /// This will not be present if the change source cannot be described by CloudFormation's
    /// limited vocabulary, such as tags supplied when creating a change set.
    ///
    /// See [`ChangeSource`] for more information.
    pub change_source: Option<ChangeSource>,

    /// Indicates whether AWS CloudFormation can determine the target value, and whether the target
    /// value will change before you execute a change set.
    ///
    /// See [`Evaluation`] for more information.
    pub evaluation: Evaluation,

    /// A structure that describes the field that AWS CloudFormation will change and whether the
    /// resource will be recreated.
    pub target: ResourceTargetDefinition,
}

impl ResourceChangeDetail {
    fn from_sdk(
        resource_type: &str,
        details: aws_sdk_cloudformation::types::ResourceChangeDetail,
    ) -> Self {
        let causing_entity = details.causing_entity;
        Self {
            change_source: details
                .change_source
                .map(move |change_source| ChangeSource::from_sdk(&change_source, causing_entity)),
            evaluation: details
                .evaluation
                .expect("ResourceChangeDetail without evaluation")
                .as_str()
                .parse()
                .expect("ResourceChangeDetail with invalid evaluation"),
            target: ResourceTargetDefinition::from_sdk(
                resource_type,
                details.target.expect("ResourceChangeDetail without target"),
            ),
        }
    }
}

/// The type of an entity that triggered a change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeSource {
    /// `Ref` intrinsic functions that refer to resources in the template, such as
    /// `{ "Ref" : "MyEC2InstanceResource" }`.
    ResourceReference(
        /// The identity of the resource that triggered the change.
        String,
    ),

    /// `Ref` intrinsic functions that get template parameter values, such as
    /// `{ "Ref" : "MyPasswordParameter" }`.
    ParameterReference(
        /// The identity of the parameter that triggered the change.
        String,
    ),

    /// `Fn::GetAtt` intrinsic functions that get resource attribute values, such as `
    /// { "Fn::GetAtt" : [ "MyEC2InstanceResource", "PublicDnsName" ] }`.
    ResourceAttribute(
        /// The identity of the resource that triggered the change.
        String,
    ),

    /// Changes that are made directly to the template.
    DirectModification,

    /// Automatic entities are `AWS::CloudFormation::Stack` resource types, which are also known as
    /// nested stacks.
    ///
    /// If you made no changes to the `AWS::CloudFormation::Stack` resource, AWS CloudFormation sets
    /// the `change_source` to `Automatic` because the nested stack's template might have changed.
    /// Changes to a nested stack's template aren't visible to AWS CloudFormation until you run an
    /// update on the parent stack.
    Automatic,
}

impl ChangeSource {
    fn from_sdk(
        change_source: &aws_sdk_cloudformation::types::ChangeSource,
        causing_entity: Option<String>,
    ) -> Self {
        match change_source {
            aws_sdk_cloudformation::types::ChangeSource::ResourceReference
            | aws_sdk_cloudformation::types::ChangeSource::ParameterReference
            | aws_sdk_cloudformation::types::ChangeSource::ResourceAttribute => {
                let causing_entity = causing_entity.unwrap_or_else(|| {
                    panic!(
                        "ResourceChangeDetail with change_source {:?} without causing_entity",
                        change_source
                    )
                });
                match change_source {
                    aws_sdk_cloudformation::types::ChangeSource::ResourceReference => {
                        Self::ResourceReference(causing_entity)
                    }
                    aws_sdk_cloudformation::types::ChangeSource::ParameterReference => {
                        Self::ParameterReference(causing_entity)
                    }
                    aws_sdk_cloudformation::types::ChangeSource::ResourceAttribute => {
                        Self::ResourceAttribute(causing_entity)
                    }
                    _ => unreachable!(),
                }
            }
            aws_sdk_cloudformation::types::ChangeSource::DirectModification => {
                Self::DirectModification
            }
            aws_sdk_cloudformation::types::ChangeSource::Automatic => Self::Automatic,
            _ => panic!(
                "ResourceChangeDetail with invalid change_source {:?}",
                change_source
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
pub enum Evaluation {
    /// AWS CloudFormation can determine that the target value will change, and its value.
    ///
    /// For example, if you directly modify the `InstanceType` property of an EC2 instance, AWS
    /// CloudFormation knows that this property value will change, and its value, so this is a
    /// `Static` evaluation.
    Static,

    /// AWS CloudFormation cannot determine the target value because it depends on the result of an
    /// intrinsic function, such as a `Ref` or `Fn::GetAtt`, when the stack is updated.
    ///
    /// For example, if your template includes a reference to a resource that is conditionally
    /// recreated, the value of the reference (the physical ID of the resource) might change,
    /// depending on if the resource is recreated. If the resource is recreated, it will have a new
    /// physical ID, so all references to that resource will also be updated.
    Dynamic,
}

/// The field that AWS CloudFormation will change, such as the name of a resource's property, and
/// whether the resource will be recreated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceTargetDefinition {
    /// A change to the resource's properties.
    Properties {
        /// The name of the property.
        name: String,

        /// Indicates whether a change to this property causes the resource to be recreated.
        requires_recreation: RequiresRecreation,
    },

    /// A change to the resource's metadata.
    Metadata,

    /// A change to the resource's creation policy.
    CreationPolicy,

    /// A change to the resource's update policy.
    UpdatePolicy,

    /// A change to the resource's deletion policy.
    DeletionPolicy,

    /// A change to the resource's tags.
    Tags,
}

impl ResourceTargetDefinition {
    fn from_sdk(
        resource_type: &str,
        target: aws_sdk_cloudformation::types::ResourceTargetDefinition,
    ) -> Self {
        let attribute = target
            .attribute
            .expect("ResourceTargetDefinition without attribute");
        match attribute {
            aws_sdk_cloudformation::types::ResourceAttribute::Properties => Self::Properties {
                name: target
                    .name
                    .expect("ResourceTargetDefinition with attribute \"Properties\" without name"),
                requires_recreation: target
                    .requires_recreation
                    .expect(concat!(
                        "ResourceTargetDefinition with attribute \"Properties\" without ",
                        "requires_recreation"
                    ))
                    .as_str()
                    .parse()
                    .expect("ResourceTargetDefinition with invalid requires_recreation"),
            },
            aws_sdk_cloudformation::types::ResourceAttribute::Metadata
            | aws_sdk_cloudformation::types::ResourceAttribute::CreationPolicy
            | aws_sdk_cloudformation::types::ResourceAttribute::UpdatePolicy
            | aws_sdk_cloudformation::types::ResourceAttribute::DeletionPolicy
            | aws_sdk_cloudformation::types::ResourceAttribute::Tags => {
                assert!(
                    target.name.is_none(),
                    "ResourceTargetDefinition with attribute {:?} with name",
                    attribute
                );
                assert!(
                    // We assume that changes to these attributes would never require recreation.
                    // NOTE: CloudFormation may report tag changes on AWS::SecretsManager::Secret
                    // resources as conditionally requiring recreation. We assume this is a bug in
                    // CloudFormation and ignore it.
                    matches!(
                        target.requires_recreation,
                        None | Some(aws_sdk_cloudformation::types::RequiresRecreation::Never)
                    ) || resource_type == "AWS::SecretsManager::Secret",
                    "ResourceTargetDefinition with attribute {:?} with requires_recreation",
                    attribute
                );
                match attribute.as_str() {
                    "Metadata" => Self::Metadata,
                    "CreationPolicy" => Self::CreationPolicy,
                    "UpdatePolicy" => Self::UpdatePolicy,
                    "DeletionPolicy" => Self::DeletionPolicy,
                    "Tags" => Self::Tags,
                    _ => unreachable!(),
                }
            }
            _ => panic!("ResourceTargetDefinition with invalid attribute"),
        }
    }
}

/// Indicates whether a change to a property causes the resource to be recreated.
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
pub enum RequiresRecreation {
    /// The resource will not need to be recreated.
    Never,

    /// The resource *may* need to be recreated.
    ///
    /// To determine the conditions for a `Conditionally` recreation, see the update behavior for
    /// that property in the AWS CloudFormation User Guide.
    Conditionally,

    /// The resource will need to be recreated.
    Always,
}

pub(crate) struct ChangeSetWithType {
    pub(crate) change_set: ChangeSet,
    pub(crate) change_set_type: ChangeSetType,
}

pub(crate) enum CreateChangeSetError {
    CreateApi(SdkError<aws_sdk_cloudformation::operation::create_change_set::CreateChangeSetError>),
    PollApi(SdkError<DescribeChangeSetError>),
    Blocked { status: BlockedStackStatus },
    NoChanges(ChangeSetWithType),
    Failed(ChangeSetWithType),
}

impl From<SdkError<aws_sdk_cloudformation::operation::create_change_set::CreateChangeSetError>>
    for CreateChangeSetError
{
    fn from(
        error: SdkError<aws_sdk_cloudformation::operation::create_change_set::CreateChangeSetError>,
    ) -> Self {
        if let Some(status) = is_create_blocked(&error) {
            Self::Blocked { status }
        } else {
            Self::CreateApi(error)
        }
    }
}

impl From<SdkError<DescribeChangeSetError>> for CreateChangeSetError {
    fn from(error: SdkError<DescribeChangeSetError>) -> Self {
        Self::PollApi(error)
    }
}

pub(crate) async fn create_change_set(
    client: &aws_sdk_cloudformation::Client,
    mut change_set_type: ChangeSetType,
    input: CreateChangeSetFluentBuilder,
) -> Result<ChangeSetWithType, CreateChangeSetError> {
    let change_set = input
        .clone()
        .send()
        .or_else({
            let change_set_type = &mut change_set_type;
            |error| async move {
                match (change_set_type, error) {
                    (change_set_type @ ChangeSetType::Create, error)
                        if is_already_exists(&error) =>
                    {
                        *change_set_type = ChangeSetType::Update;
                        input
                            .change_set_type(change_set_type.into_sdk())
                            .send()
                            .await
                    }
                    (_, error) => Err(error),
                }
            }
        })
        .await?;
    let change_set_id = change_set.id.expect("CreateChangeSetOutput without id");

    let mut interval = interval_at(
        Instant::now() + POLL_INTERVAL_CHANGE_SET,
        POLL_INTERVAL_CHANGE_SET,
    );
    loop {
        interval.tick().await;

        let change_set = client
            .describe_change_set()
            .change_set_name(change_set_id.clone())
            .send()
            .await?;
        let change_set = ChangeSet::from_sdk(change_set);
        match change_set.status {
            ChangeSetStatus::CreatePending | ChangeSetStatus::CreateInProgress => continue,
            ChangeSetStatus::CreateComplete => {
                return Ok(ChangeSetWithType {
                    change_set,
                    change_set_type,
                })
            }
            ChangeSetStatus::Failed if is_no_changes(change_set.status_reason.as_deref()) => {
                return Err(CreateChangeSetError::NoChanges(ChangeSetWithType {
                    change_set,
                    change_set_type,
                }))
            }
            ChangeSetStatus::Failed => {
                return Err(CreateChangeSetError::Failed(ChangeSetWithType {
                    change_set,
                    change_set_type,
                }))
            }
            _ => {
                panic!(
                    "change set {} had unexpected status: {}",
                    change_set.change_set_id, change_set.status
                );
            }
        }
    }
}

pub(crate) enum ExecuteChangeSetError {
    ExecuteApi(
        SdkError<aws_sdk_cloudformation::operation::execute_change_set::ExecuteChangeSetError>,
    ),
    Blocked {
        status: BlockedStackStatus,
    },
}

impl From<SdkError<aws_sdk_cloudformation::operation::execute_change_set::ExecuteChangeSetError>>
    for ExecuteChangeSetError
{
    fn from(
        error: SdkError<
            aws_sdk_cloudformation::operation::execute_change_set::ExecuteChangeSetError,
        >,
    ) -> Self {
        Self::ExecuteApi(error)
    }
}

pub(crate) async fn execute_change_set(
    client: &aws_sdk_cloudformation::Client,
    stack_id: String,
    change_set_id: String,
    change_set_type: ChangeSetType,
    disable_rollback: bool,
) -> Result<
    StackOperation<'_, impl Fn(StackStatus) -> StackOperationStatus + Unpin>,
    ExecuteChangeSetError,
> {
    let started_at = Utc::now();
    client
        .execute_change_set()
        .set_disable_rollback(Some(disable_rollback))
        .change_set_name(change_set_id)
        .send()
        .await
        .map_err(|error| {
            if let Some(status) = is_execute_blocked(&error) {
                return ExecuteChangeSetError::Blocked { status };
            }
            ExecuteChangeSetError::ExecuteApi(error)
        })?;

    Ok(StackOperation::new(
        client,
        stack_id,
        started_at,
        match change_set_type {
            ChangeSetType::Create => check_create_progress,
            ChangeSetType::Update => check_update_progress,
        },
    ))
}

fn is_already_exists(
    error: &SdkError<aws_sdk_cloudformation::operation::create_change_set::CreateChangeSetError>,
) -> bool {
    error
        .message()
        .is_some_and(|msg| msg.contains(" already exists "))
}

fn is_create_blocked(
    error: &SdkError<aws_sdk_cloudformation::operation::create_change_set::CreateChangeSetError>,
) -> Option<BlockedStackStatus> {
    lazy_static::lazy_static! {
        static ref BLOCKED: regex::Regex = regex::Regex::new(r"(?i)^Stack:[^ ]* is in (?P<status>[_A-Z]+) state and can not be updated").unwrap();
    }

    is_blocked(&BLOCKED, error.message().unwrap())
}

fn is_execute_blocked(
    error: &SdkError<aws_sdk_cloudformation::operation::execute_change_set::ExecuteChangeSetError>,
) -> Option<BlockedStackStatus> {
    lazy_static::lazy_static! {
        static ref BLOCKED: regex::Regex = regex::Regex::new(r"(?i)^This stack is currently in a non-terminal \[(?P<status>[_A-Z]+)\] state").unwrap();
    }

    is_blocked(&BLOCKED, error.message().unwrap())
}

fn is_blocked(pattern: &Regex, message: &str) -> Option<BlockedStackStatus> {
    let detail = pattern.captures(message)?;

    let status: StackStatus = detail
        .name("status")
        .unwrap()
        .as_str()
        .parse()
        .expect("captured invalid status");
    let status = BlockedStackStatus::try_from(status).expect("captured non-blocked status");
    Some(status)
}

fn is_no_changes(status_reason: Option<&str>) -> bool {
    let status_reason = status_reason.unwrap_or_default();
    status_reason.contains("The submitted information didn't contain changes.")
        || status_reason.contains("No updates are to be performed.")
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
        StackStatus::UpdateFailed
        | StackStatus::UpdateRollbackFailed
        | StackStatus::UpdateRollbackComplete => StackOperationStatus::Failed,
        _ => StackOperationStatus::Unexpected,
    }
}
