#![warn(clippy::pedantic)]

mod apply_stack;
mod delete_stack;
mod event;
mod stack;
mod status;
mod tag;

pub mod change_set;
pub mod status_reason;

use aws_config::SdkConfig;

pub use apply_stack::{
    ApplyStack, ApplyStackChangeSet, ApplyStackError, ApplyStackEvents, ApplyStackInput,
    ApplyStackOutput, Capability, Parameter, StackOutput, TemplateSource,
};
pub use delete_stack::{DeleteStack, DeleteStackError, DeleteStackEvents, DeleteStackInput};
pub use event::{StackEvent, StackEventDetails};
pub use stack::{StackFailure, StackWarning};
pub use status::{
    ChangeSetStatus, InvalidStatus, ResourceStatus, StackStatus, Status, StatusSentiment,
};
pub use tag::Tag;

/// A client for performing cloudformatious operations.
pub struct Client {
    inner: aws_sdk_cloudformation::Client,
}

impl Client {
    /// Construct a new client for the given AWS SDK configuration.
    #[must_use]
    pub fn new(config: &SdkConfig) -> Self {
        Self {
            inner: aws_sdk_cloudformation::Client::new(config),
        }
    }

    /// Apply a CloudFormation stack to an AWS environment.
    ///
    /// This is an idempotent operation that will create the indicated stack if it doesn't exist, or
    /// update it if it does. It is not an error for there to be no changes.
    ///
    /// This is similar to the `aws cloudformation deploy` command from the AWS CLI (with
    /// `--no-fail-on-empty-changeset` always on).
    ///
    /// The returned `Future` can be used to simply wait for the operation to complete. You can also
    /// use [`ApplyStack::events`] to get a `Stream` of the stack events that occur during the
    /// operation. See [`ApplyStack`] for more details.
    #[must_use]
    pub fn apply_stack(&self, input: ApplyStackInput) -> ApplyStack {
        ApplyStack::new(&self.inner, input)
    }

    /// Delete a CloudFormation stack from an AWS environment.
    ///
    /// This is an idempotent operation that will delete the indicated stack if it exists, or do
    /// nothing if it does not.
    ///
    /// [`DeleteStack::events`] can be used to get a `Stream` of `StackEvent`s that occur during
    /// deletion (the stream will be empty if the stack does not exist). See the [`DeleteStack`]
    /// struct for more details.
    #[must_use]
    pub fn delete_stack(&self, input: DeleteStackInput) -> DeleteStack {
        DeleteStack::new(&self.inner, input)
    }
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
