#![warn(clippy::pedantic)]

mod apply;
mod change_set;
mod event;
mod status;

pub mod raw;

use rusoto_cloudformation::{CloudFormation, CloudFormationClient};

pub use apply::{
    Apply, ApplyError, ApplyEvent, ApplyInput, ApplyOutput, Capability, Parameter, StackOutput,
};
pub use event::{StackEvent, StackEventDetails};
pub use status::{
    ChangeSetStatus, InvalidStatus, ResourceStatus, StackStatus, Status, StatusSentiment,
};

/// High-level CloudFormation operations.
pub trait CloudFormationExt: CloudFormation + Sized + private::Sealed {
    /// Apply a CloudFormation template to an AWS environment.
    ///
    /// This is an idempotent operation that will create the indicated stack if it doesn't exist, or
    /// update it if it does. It is not an error for there to be no changes.
    ///
    /// The return value implements both `Future` and `Stream`. The `Future` implementation can be
    /// used to simply wait for the operation to complete, or the `Stream` implementation can be
    /// used to react to stack events that occur during the operation. See the [`Apply`] struct for
    /// more details.
    fn apply(&self, input: ApplyInput) -> Apply {
        Apply::new(self, input)
    }
}

impl CloudFormationExt for CloudFormationClient {}

mod private {
    use rusoto_cloudformation::CloudFormationClient;

    /// An unreachable trait used to prevent some traits from being implemented outside the crate.
    pub trait Sealed {}

    impl Sealed for CloudFormationClient {}
}
