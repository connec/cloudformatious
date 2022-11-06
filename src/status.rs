//! Types and values representing various CloudFormation statuses.
#![allow(clippy::module_name_repetitions)]

/// Common operations for statuses.
pub trait Status: std::fmt::Debug + std::fmt::Display + private::Sealed {
    /// Indicates whether or not a status is settled.
    ///
    /// A settled status is one that won't change again during the current stack operation.
    fn is_settled(&self) -> bool;

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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusSentiment {
    Positive,
    Neutral,
    Negative,
}

impl StatusSentiment {
    /// Is the sentiment positive?
    #[must_use]
    pub fn is_positive(self) -> bool {
        self == Self::Positive
    }

    /// Is the sentiment neutral?
    #[must_use]
    pub fn is_neutral(self) -> bool {
        self == Self::Neutral
    }

    /// Is the sentiment negative?
    #[must_use]
    pub fn is_negative(self) -> bool {
        self == Self::Negative
    }
}

/// Possible change set statuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
#[display(style = "SNAKE_CASE")]
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
    fn is_settled(&self) -> bool {
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

/// Possible stack statuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
#[display(style = "SNAKE_CASE")]
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
    fn is_settled(&self) -> bool {
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

/// Possible resource statuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq, parse_display::Display, parse_display::FromStr)]
#[display(style = "SNAKE_CASE")]
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
    fn is_settled(&self) -> bool {
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
        assert!("oh no".parse::<StackStatus>().is_err());
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
        assert!("oh no".parse::<ResourceStatus>().is_err());
    }
}
