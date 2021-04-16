#![warn(clippy::pedantic)]

mod event;
mod status;

pub mod raw;

pub use event::{StackEvent, StackEventDetails};
pub use status::{InvalidStatus, ResourceStatus, StackStatus, Status, StatusSentiment};
