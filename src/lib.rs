#![warn(clippy::pedantic)]

mod apply;
mod change_set;
mod event;
mod status;

use rusoto_cloudformation::{CloudFormation, CloudFormationClient};

pub use apply::{
    Apply, ApplyError, ApplyInput, ApplyOutput, Capability, Parameter, StackOutput, TemplateSource,
};
pub use event::{StackEvent, StackEventDetails};
pub use status::{
    ChangeSetStatus, InvalidStatus, ResourceStatus, StackStatus, Status, StatusSentiment,
};

/// High-level CloudFormation operations.
pub trait CloudFormatious: CloudFormation + Sized + private::Sealed {
    /// Apply a CloudFormation template to an AWS environment.
    ///
    /// This is an idempotent operation that will create the indicated stack if it doesn't exist, or
    /// update it if it does. It is not an error for there to be no changes.
    ///
    /// This is similar to the `aws cloudformation deploy` command from the AWS CLI (with
    /// `--no-fail-on-empty-changeset` always on).
    ///
    /// The return value implements both `Future` and `Stream`. The `Future` implementation can be
    /// used to simply wait for the operation to complete, or the `Stream` implementation can be
    /// used to react to stack events that occur during the operation. See the [`Apply`] struct for
    /// more details.
    fn apply(&self, input: ApplyInput) -> Apply {
        Apply::new(self, input)
    }
}

impl CloudFormatious for CloudFormationClient {}

mod private {
    use rusoto_cloudformation::CloudFormationClient;

    /// An unreachable trait used to prevent some traits from being implemented outside the crate.
    pub trait Sealed {}

    impl Sealed for CloudFormationClient {}
}

#[cfg(doctest)]
mod test_readme {
    macro_rules! external_doc_test {
        ($x:expr) => {
            #[doc = $x]
            extern "C" {}
        };
    }

    external_doc_test!(include_str!("../README.md"));
}
