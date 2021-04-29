use std::fmt;

use crate::{ResourceStatus, StackEventDetails, StackStatus};

/// Describes a failed stack operation.
///
/// This error tries to capture enough information to quickly identify the root-cause of the
/// operation's failure (such as not having permission to create or update a particular resource
/// in the stack).
#[derive(Debug)]
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
pub struct StackWarning {
    /// The ID of the stack.
    pub stack_id: String,

    /// Resource events with negative statuses that did not affect the overall operation.
    ///
    /// **Note:** this is represented as a `Vec` or tuples to avoid having to worry about
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
