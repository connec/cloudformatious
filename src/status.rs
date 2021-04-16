//! Types and values representing various CloudFormation statuses.
#![allow(clippy::module_name_repetitions)]

use std::str::FromStr;

use serde_plain::forward_display_to_serde;

/// Common operations for statuses.
pub trait Status: std::fmt::Debug + std::fmt::Display + private::Sealed {
    /// Indicates whether or not a status is terminal.
    ///
    /// A terminal status is one that won't change again during the current stack operation.
    fn is_terminal(&self) -> bool;

    /// Indicates the sentiment of the status.
    ///
    /// This is obviously a bit fuzzy, but in general:
    ///
    /// - Successful terminal statuses are positive.
    /// - Failed terminal statuses and rollback statuses are negative.
    /// - All other statuses are neutral.
    fn sentiment(&self) -> StatusSentiment;
}

/// An indication of whether a status is positive, negative, or neutral for the affected resource.
pub enum StatusSentiment {
    Positive,
    Neutral,
    Negative,
}

/// An error marker returned when trying to parse an invalid status.
#[derive(Debug, Eq, PartialEq)]
pub struct InvalidStatus;

/// Possible change set statuses.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeSetStatus {
    CreatePending,
    CreateInProgress,
    CreateComplete,
    DeletePending,
    DeleteInProgress,
    DeleteComplete,
    DeleteFailed,
    Failed,
}

impl Status for ChangeSetStatus {
    fn is_terminal(&self) -> bool {
        match self {
            Self::CreatePending
            | Self::CreateInProgress
            | Self::DeletePending
            | Self::DeleteInProgress => false,
            Self::CreateComplete | Self::DeleteComplete | Self::DeleteFailed | Self::Failed => true,
        }
    }

    fn sentiment(&self) -> StatusSentiment {
        match self {
            Self::CreateComplete | Self::DeleteComplete => StatusSentiment::Positive,
            Self::CreatePending
            | Self::CreateInProgress
            | Self::DeletePending
            | Self::DeleteInProgress => StatusSentiment::Neutral,
            Self::DeleteFailed | Self::Failed => StatusSentiment::Negative,
        }
    }
}

forward_display_to_serde!(ChangeSetStatus);

impl FromStr for ChangeSetStatus {
    type Err = InvalidStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_plain::from_str(s).map_err(|_| InvalidStatus)
    }
}

/// Possible stack statuses.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StackStatus {
    CreateInProgress,
    CreateFailed,
    CreateComplete,
    RollbackInProgress,
    RollbackFailed,
    RollbackComplete,
    DeleteInProgress,
    DeleteFailed,
    DeleteComplete,
    UpdateInProgress,
    UpdateCompleteCleanupInProgress,
    UpdateComplete,
    UpdateRollbackInProgress,
    UpdateRollbackFailed,
    UpdateRollbackCompleteCleanupInProgress,
    UpdateRollbackComplete,
    ReviewInProgress,
    ImportInProgress,
    ImportComplete,
    ImportRollbackInProgress,
    ImportRollbackFailed,
    ImportRollbackComplete,
}

impl Status for StackStatus {
    fn is_terminal(&self) -> bool {
        match self {
            Self::CreateInProgress
            | Self::RollbackInProgress
            | Self::DeleteInProgress
            | Self::UpdateInProgress
            | Self::UpdateCompleteCleanupInProgress
            | Self::UpdateRollbackInProgress
            | Self::UpdateRollbackCompleteCleanupInProgress
            | Self::ReviewInProgress
            | Self::ImportInProgress
            | Self::ImportRollbackInProgress => false,
            Self::CreateFailed
            | Self::CreateComplete
            | Self::RollbackFailed
            | Self::RollbackComplete
            | Self::DeleteFailed
            | Self::DeleteComplete
            | Self::UpdateComplete
            | Self::UpdateRollbackFailed
            | Self::UpdateRollbackComplete
            | Self::ImportComplete
            | Self::ImportRollbackFailed
            | Self::ImportRollbackComplete => true,
        }
    }

    fn sentiment(&self) -> StatusSentiment {
        match self {
            Self::CreateComplete
            | Self::DeleteComplete
            | Self::UpdateComplete
            | Self::ImportComplete => StatusSentiment::Positive,
            Self::CreateInProgress
            | Self::DeleteInProgress
            | Self::UpdateInProgress
            | Self::UpdateCompleteCleanupInProgress
            | Self::ReviewInProgress
            | Self::ImportInProgress => StatusSentiment::Neutral,
            Self::CreateFailed
            | Self::RollbackInProgress
            | Self::RollbackFailed
            | Self::RollbackComplete
            | Self::DeleteFailed
            | Self::UpdateRollbackInProgress
            | Self::UpdateRollbackFailed
            | Self::UpdateRollbackCompleteCleanupInProgress
            | Self::UpdateRollbackComplete
            | Self::ImportRollbackInProgress
            | Self::ImportRollbackFailed
            | Self::ImportRollbackComplete => StatusSentiment::Negative,
        }
    }
}

forward_display_to_serde!(StackStatus);

impl FromStr for StackStatus {
    type Err = InvalidStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_plain::from_str(s).map_err(|_| InvalidStatus)
    }
}

/// Possible resource statuses.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResourceStatus {
    CreateInProgress,
    CreateFailed,
    CreateComplete,
    DeleteInProgress,
    DeleteFailed,
    DeleteComplete,
    DeleteSkipped,
    UpdateInProgress,
    UpdateFailed,
    UpdateComplete,
    ImportFailed,
    ImportComplete,
    ImportInProgress,
    ImportRollbackInProgress,
    ImportRollbackFailed,
    ImportRollbackComplete,
}

impl Status for ResourceStatus {
    fn is_terminal(&self) -> bool {
        match self {
            Self::CreateInProgress
            | Self::DeleteInProgress
            | Self::UpdateInProgress
            | Self::ImportInProgress
            | Self::ImportRollbackInProgress => false,
            Self::CreateFailed
            | Self::CreateComplete
            | Self::DeleteFailed
            | Self::DeleteComplete
            | Self::DeleteSkipped
            | Self::UpdateFailed
            | Self::UpdateComplete
            | Self::ImportFailed
            | Self::ImportComplete
            | Self::ImportRollbackFailed
            | Self::ImportRollbackComplete => true,
        }
    }

    fn sentiment(&self) -> StatusSentiment {
        match self {
            Self::CreateComplete
            | Self::DeleteComplete
            | Self::UpdateComplete
            | Self::ImportComplete => StatusSentiment::Positive,
            Self::CreateInProgress
            | Self::DeleteInProgress
            | Self::DeleteSkipped
            | Self::UpdateInProgress
            | Self::ImportInProgress => StatusSentiment::Neutral,
            Self::CreateFailed
            | Self::DeleteFailed
            | Self::UpdateFailed
            | Self::ImportFailed
            | Self::ImportRollbackInProgress
            | Self::ImportRollbackFailed
            | Self::ImportRollbackComplete => StatusSentiment::Negative,
        }
    }
}

forward_display_to_serde!(ResourceStatus);

impl FromStr for ResourceStatus {
    type Err = InvalidStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_plain::from_str(s).map_err(|_| InvalidStatus)
    }
}

mod private {
    /// An unreachable trait used to prevent some traits from being implemented outside the crate.
    pub trait Sealed {}

    impl Sealed for super::ChangeSetStatus {}
    impl Sealed for super::StackStatus {}
    impl Sealed for super::ResourceStatus {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_set_status() {
        // there's no point testing every variant, but we should check one to be sure.
        assert_eq!(
            format!("{}", ChangeSetStatus::CreateInProgress).as_str(),
            "CREATE_IN_PROGRESS"
        );
        assert_eq!(
            "CREATE_IN_PROGRESS".parse(),
            Ok(ChangeSetStatus::CreateInProgress)
        );
        assert_eq!("oh no".parse::<ChangeSetStatus>(), Err(InvalidStatus));
    }

    #[test]
    fn stack_status() {
        // there's no point testing every variant, but we should check one to be sure.
        assert_eq!(
            format!("{}", StackStatus::UpdateRollbackCompleteCleanupInProgress).as_str(),
            "UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS"
        );
        assert_eq!(
            "UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS".parse(),
            Ok(StackStatus::UpdateRollbackCompleteCleanupInProgress)
        );
        assert_eq!("oh no".parse::<StackStatus>(), Err(InvalidStatus));
    }

    #[test]
    fn resource_status() {
        // there's no point testing every variant, but we should check one to be sure.
        assert_eq!(
            format!("{}", ResourceStatus::ImportRollbackInProgress).as_str(),
            "IMPORT_ROLLBACK_IN_PROGRESS"
        );
        assert_eq!(
            "IMPORT_ROLLBACK_IN_PROGRESS".parse(),
            Ok(ResourceStatus::ImportRollbackInProgress)
        );
        assert_eq!("oh no".parse::<ResourceStatus>(), Err(InvalidStatus));
    }
}
