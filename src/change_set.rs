//! Helpers for working with change sets.

use std::{fmt, time::Duration};

use chrono::{DateTime, Utc};
use enumset::EnumSet;
use futures_util::TryFutureExt;
use memmem::{Searcher, TwoWaySearcher};
use rusoto_cloudformation::{
    Change, CloudFormation, CreateChangeSetInput, DescribeChangeSetError, DescribeChangeSetInput,
    DescribeChangeSetOutput, ExecuteChangeSetError, ExecuteChangeSetInput, Parameter, Tag,
};
use rusoto_core::{request::BufferedHttpResponse, RusotoError};
use serde_plain::{forward_display_to_serde, forward_from_str_to_serde};
use tokio::time::{interval_at, Instant};

use crate::{
    stack::{StackOperation, StackOperationStatus},
    Capability, ChangeSetStatus, StackStatus,
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

/// A planned set of changes to apply to a CloudFormation stack.
#[derive(Clone, Debug, PartialEq)]
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
    fn from_raw(change_set: DescribeChangeSetOutput) -> Self {
        Self {
            capabilities: change_set
                .capabilities
                .unwrap_or_default()
                .into_iter()
                .map(|capability| {
                    capability
                        .parse()
                        .expect("DescribeChangeSetOutput with invalid capability")
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
                .map(ResourceChange::from_raw)
                .collect(),
            creation_time: DateTime::parse_from_rfc3339(
                &change_set
                    .creation_time
                    .expect("DescribeChangeSetOutput without creation_time"),
            )
            .expect("DescribeChangeSetOutput invalid creation_time")
            .into(),
            description: change_set.description,
            execution_status: change_set
                .execution_status
                .expect("DescribeChangeSetOutput without execution_status")
                .parse()
                .expect("DescribeChangeSetOutput with invalid execution_status"),
            notification_arns: change_set.notification_ar_ns.unwrap_or_default(),
            parameters: change_set.parameters.unwrap_or_default(),
            stack_id: change_set
                .stack_id
                .expect("DescribeChangeSetOutput without stack_id"),
            stack_name: change_set
                .stack_name
                .expect("DescribeChangeSetOutput without stack_name"),
            status: change_set
                .status
                .expect("DescribeChangeSetOutput without status")
                .parse()
                .expect("DescribeChangeSetOutput unexpected status"),
            status_reason: change_set.status_reason,
            tags: change_set.tags.unwrap_or_default(),
        }
    }
}

/// The change set's execution status.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
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

forward_display_to_serde!(ExecutionStatus);
forward_from_str_to_serde!(ExecutionStatus);

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
    fn from_raw(change: Change) -> Self {
        if change.type_.as_deref() != Some("Resource") {
            panic!("Change with unexpected type_ {:?}", change.type_);
        }
        let change = change
            .resource_change
            .expect("Change without resource_change");
        Self {
            action: Action::from_raw(
                change
                    .action
                    .as_deref()
                    .expect("ResourceChange without action"),
                change.details,
                change.replacement,
                change.scope,
            ),
            logical_resource_id: change
                .logical_resource_id
                .expect("ResourceChange without logical_resource_id"),
            physical_resource_id: change.physical_resource_id,
            resource_type: change
                .resource_type
                .expect("ResourceChange without resource_type"),
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
    fn from_raw(
        action: &str,
        details: Option<Vec<rusoto_cloudformation::ResourceChangeDetail>>,
        replacement: Option<String>,
        scope: Option<Vec<String>>,
    ) -> Self {
        match action {
            "Add" | "Remove" | "Import" | "Dynamic" => {
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
                    "Add" => Self::Add,
                    "Remove" => Self::Remove,
                    "Import" => Self::Import,
                    "Dynamic" => Self::Dynamic,
                    _ => unreachable!(),
                }
            }
            "Modify" => Self::Modify(ModifyDetail::from_raw(
                details.expect("ResourceChange with action \"Modify\" without details"),
                &replacement.expect("ResourceChange with action \"Modify\" without replacement"),
                &scope.expect("ResourceChange with action \"Modify\" without scope"),
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
    fn from_raw(
        details: Vec<rusoto_cloudformation::ResourceChangeDetail>,
        replacement: &str,
        scope: &[String],
    ) -> Self {
        Self {
            details: details
                .into_iter()
                .map(ResourceChangeDetail::from_raw)
                .collect(),
            replacement: replacement
                .parse()
                .expect("ResourceChange with invalid replacement"),
            scope: scope
                .iter()
                .map(|scope| -> ModifyScope {
                    scope.parse().expect("ResourceChange with invalid scope")
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Replacement {
    /// The resource will be replaced.
    True,

    /// The resource will not be replaced.
    False,

    /// The resource *may* be replaced.
    Conditional,
}

forward_display_to_serde!(Replacement);
forward_from_str_to_serde!(Replacement);

// The derive for EnumSetType creates an item that triggers this lint, so it has to be disabled
// at the module level. We don't want to disable it too broadly though, so we wrap its declaration
// in a module and re-export from that.
mod modify_scope {
    #![allow(clippy::expl_impl_clone_on_copy)]

    /// Indicates which resource attribute is triggering this update.
    #[derive(Debug, enumset::EnumSetType, serde::Deserialize, serde::Serialize)]
    #[enumset(no_ops, serialize_as_list)]
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

forward_display_to_serde!(ModifyScope);
forward_from_str_to_serde!(ModifyScope);

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
    fn from_raw(details: rusoto_cloudformation::ResourceChangeDetail) -> Self {
        let causing_entity = details.causing_entity;
        Self {
            change_source: details
                .change_source
                .as_deref()
                .map(move |change_source| ChangeSource::from_raw(change_source, causing_entity)),
            evaluation: details
                .evaluation
                .expect("ResourceChangeDetail without evaluation")
                .parse()
                .expect("ResourceChangeDetail with invalid evaluation"),
            target: ResourceTargetDefinition::from_raw(
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
    fn from_raw(change_source: &str, causing_entity: Option<String>) -> Self {
        match change_source {
            "ResourceReference" | "ParameterReference" | "ResourceAttribute" => {
                let causing_entity = causing_entity.unwrap_or_else(|| {
                    panic!(
                        "ResourceChangeDetail with change_source {:?} without causing_entity",
                        change_source
                    )
                });
                match change_source {
                    "ResourceReference" => Self::ResourceReference(causing_entity),
                    "ParameterReference" => Self::ParameterReference(causing_entity),
                    "ResourceAttribute" => Self::ResourceAttribute(causing_entity),
                    _ => unreachable!(),
                }
            }
            "DirectModification" => Self::DirectModification,
            "Automatic" => Self::Automatic,
            _ => panic!(
                "ResourceChangeDetail with invalid change_source {:?}",
                change_source
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
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

forward_display_to_serde!(Evaluation);
forward_from_str_to_serde!(Evaluation);

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
    fn from_raw(target: rusoto_cloudformation::ResourceTargetDefinition) -> Self {
        let attribute = target
            .attribute
            .expect("ResourceTargetDefinition without attribute");
        match attribute.as_str() {
            "Properties" => {
                Self::Properties {
                    name: target.name.expect("ResourceTargetDefinition with attribute \"Properties\" without name"),
                    requires_recreation: target.requires_recreation.expect("ResourceTargetDefinition with attribute \"Properties\" without requires_recreation").parse().expect("ResourceTargetDefinition with invalid requires_recreation"),
                }
            }
            "Metadata" | "CreationPolicy" | "UpdatePolicy" | "DeletionPolicy" | "Tags" => {
                assert!(
                    target.name.is_none(),
                    "ResourceTargetDefinition with attribute {:?} with name",
                    attribute
                );
                assert!(
                    // We assume that changes to these attributes would never require recreation.
                    matches!(target.requires_recreation.as_deref(), None | Some("Never")),
                    "ResourceTargetDefinition with attribute {:?} with requires_recreation",
                    attribute
                );
                match attribute.as_str() {
                    "Metadata" => Self::Metadata,
                    "CreationPolicy" => Self::CreationPolicy,
                    "UpdatePolicy" => Self::UpdatePolicy,
                    "DeletionPolicy" => Self::DeletionPolicy,
                    "Tags" => Self::Tags,
                    _ => unreachable!()
                }
            },
            _ => panic!("ResourceTargetDefinition with invalid attribute")
        }
    }
}

/// Indicates whether a change to a property causes the resource to be recreated.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
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

forward_display_to_serde!(RequiresRecreation);
forward_from_str_to_serde!(RequiresRecreation);

pub(crate) struct ChangeSetWithType {
    pub(crate) change_set: ChangeSet,
    pub(crate) change_set_type: ChangeSetType,
}

pub(crate) enum CreateChangeSetError {
    CreateApi(RusotoError<rusoto_cloudformation::CreateChangeSetError>),
    PollApi(RusotoError<DescribeChangeSetError>),
    NoChanges(ChangeSetWithType),
    Failed(ChangeSetWithType),
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
) -> Result<ChangeSetWithType, CreateChangeSetError> {
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
        let change_set = ChangeSet::from_raw(change_set);
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

pub(crate) async fn execute_change_set<Client: CloudFormation>(
    client: &Client,
    stack_id: String,
    change_set_id: String,
    change_set_type: ChangeSetType,
) -> Result<
    StackOperation<'_, impl Fn(StackStatus) -> StackOperationStatus + Unpin>,
    RusotoError<ExecuteChangeSetError>,
> {
    let started_at = Utc::now();
    let input = ExecuteChangeSetInput {
        change_set_name: change_set_id,
        ..ExecuteChangeSetInput::default()
    };
    client.execute_change_set(input).await?;

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
