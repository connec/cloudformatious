//! Detailed status reasons.

use std::fmt;

use aws_config::SdkConfig;
use aws_sdk_sts::{
    error::SdkError, operation::decode_authorization_message::DecodeAuthorizationMessageError,
};
use lazy_static::lazy_static;
use regex::Regex;

/// A wrapper around a status reason that offers additional detail.
///
/// This is the return type of [`StackEventDetails::resource_status_reason`][1]. The [`detail`][2]
/// method will attempt to parse the inner status reason into [`StatusReasonDetail`], which can
/// indicate what specifically went wrong. The underlying status reason can be retrieved via
/// [`inner`][3].
///
/// [1]: crate::StackEventDetails::resource_status_reason
/// [2]: Self::detail
/// [3]: Self::inner
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusReason<'a>(Option<&'a str>);

impl<'a> StatusReason<'a> {
    pub(crate) fn new(status_reason: Option<&'a str>) -> Self {
        Self(status_reason)
    }

    /// The raw status reason, in case you need to work with it directly.
    #[must_use]
    pub fn inner(&self) -> Option<&'a str> {
        self.0
    }

    /// Additional detail about the status reason, if available.
    ///
    /// This currently depends on some preset parsing of the status reason string for various common
    /// error reasons. See [`StatusReasonDetail`] for current possibilities.
    pub fn detail(&self) -> Option<StatusReasonDetail<'a>> {
        self.0.and_then(StatusReasonDetail::new)
    }
}

/// Additional detail about a status reason.
#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StatusReasonDetail<'a> {
    /// Resource creation was cancelled, typically due to a preceding failure.
    CreationCancelled,

    /// The CloudFormation principal did not have permission to perform an operation.
    ///
    /// This is similar to [`AuthorizationFailure`](Self::AuthorizationFailure) but provides some
    /// information without needing to decode the failure message (if any).
    MissingPermission(MissingPermission<'a>),

    /// The CloudFormation principal was not authorized to perform an operation.
    ///
    /// This is similar to [`MissingPermission`](Self::MissingPermission) but provides no
    /// information without decoding the failure message (if any). AWS does this with the reasoning
    /// that details of why authorization failed might be sensitive. You can decode the message with
    /// [`EncodedAuthorizationMessage::decode`].
    AuthorizationFailure(EncodedAuthorizationMessage<'a>),

    /// A stack operation failed due to resource errors.
    ResourceErrors(ResourceErrors<'a>),
}

impl<'a> StatusReasonDetail<'a> {
    fn new(status_reason: &'a str) -> Option<Self> {
        lazy_static! {
            static ref CREATION_CANCELLED: Regex =
                Regex::new(r"(?i)Resource creation cancelled").unwrap();

            static ref MISSING_PERMISSION_1: Regex =
                Regex::new(r"(?i)API: (?P<permission>[a-z0-9]+:[a-z0-9]+)\b").unwrap();

            static ref MISSING_PERMISSION_2: Regex =
                Regex::new(r"(?i)User: (?P<principal>[a-z0-9:/-]+) is not authorized to perform: (?P<permission>[a-z0-9]+:[a-z0-9]+)").unwrap();

            static ref RESOURCE_ERRORS: Regex =
                Regex::new(r"(?i)The following resource\(s\) failed to (?:create|delete|update): \[(?P<logical_resource_ids>[a-z0-9]+(?:, *[a-z0-9]+)*)\]").unwrap();

            static ref ENCODED_AUTHORIZATION_MESSAGE: Regex =
                Regex::new("(?i)Encoded authorization failure message: (?P<encoded_authorization_message>[a-z0-9_-]+)").unwrap();
        }

        if CREATION_CANCELLED.is_match(status_reason) {
            return Some(Self::CreationCancelled);
        }

        let encoded_authorization_message = ENCODED_AUTHORIZATION_MESSAGE
            .captures(status_reason)
            .map(|captures| {
                EncodedAuthorizationMessage::new(
                    captures
                        .name("encoded_authorization_message")
                        .unwrap()
                        .as_str(),
                )
            });
        if let Some(detail) = MISSING_PERMISSION_1.captures(status_reason) {
            return Some(Self::MissingPermission(MissingPermission {
                permission: detail.name("permission").unwrap().as_str(),
                principal: None,
                encoded_authorization_message,
            }));
        }
        if let Some(detail) = MISSING_PERMISSION_2.captures(status_reason) {
            return Some(Self::MissingPermission(MissingPermission {
                permission: detail.name("permission").unwrap().as_str(),
                principal: Some(detail.name("principal").unwrap().as_str()),
                encoded_authorization_message,
            }));
        }
        if let Some(encoded_authorization_message) = encoded_authorization_message {
            return Some(Self::AuthorizationFailure(encoded_authorization_message));
        }

        if let Some(detail) = RESOURCE_ERRORS.captures(status_reason) {
            return Some(Self::ResourceErrors(ResourceErrors {
                logical_resource_ids: detail.name("logical_resource_ids").unwrap().as_str(),
            }));
        }
        None
    }
}

/// The CloudFormation principal did not have permission to perform an operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingPermission<'a> {
    /// The IAM permission that was missing.
    pub permission: &'a str,

    /// The CloudFormation principal.
    ///
    /// This is not reported by all missing permission status reasons, and so may not be known. If
    /// you controlled the stack operation invocation you could still determine this either from the
    /// `RoleArn` input parameter, or else the principal that started the operation.
    pub principal: Option<&'a str>,

    /// An encoded authorization failure message included in the status reason.
    pub encoded_authorization_message: Option<EncodedAuthorizationMessage<'a>>,
}

/// An encoded authorization failure message.
///
/// The message is encoded because the details of the authorization status can constitute privileged
/// information that the user who requested the operation should not see. To decode an authorization
/// status message, a user must be granted permissions via an IAM policy to request the
/// `DecodeAuthorizationMessage` (`sts:DecodeAuthorizationMessage`) action.
///
/// You can decode the message using [`decode`](Self::decode).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedAuthorizationMessage<'a>(&'a str);

impl<'a> EncodedAuthorizationMessage<'a> {
    pub(crate) fn new(message: &'a str) -> Self {
        Self(message)
    }

    /// The raw encoded authorization message, in case you need to work with it directly.
    #[must_use]
    pub fn inner(&self) -> &'a str {
        self.0
    }

    /// Decode the authorization message.
    ///
    /// This involves invoking the `sts:DecodeAuthorizationMessage` API, so an STS client is
    /// required and will need permission to invoke the API.
    ///
    /// The decoded message includes the following type of information:
    ///
    /// - Whether the request was denied due to an explicit deny or due to the absence of an
    ///   explicit allow.
    /// - The principal who made the request.
    /// - The requested action.
    /// - The requested resource.
    /// - The values of condition keys in the context of the user's request.
    ///
    /// Note that the structure of the message is not fully specified, so for now it is returned as
    /// JSON. This may change in future.
    ///
    /// # Errors
    ///
    /// Any errors encountered when invoking the `sts:DecodeAuthorizationMessage` API are returned.
    ///
    /// # Panics
    ///
    /// This will panic if the `sts:DecodeAuthorizationMessage` API does not repond with a decoded
    /// message (this is allowed by AWS SDK types but should never happen per the semantics of the
    /// API).
    pub async fn decode(
        &self,
        config: &SdkConfig,
    ) -> Result<serde_json::Value, EncodedAuthorizationMessageDecodeError> {
        let sts = aws_sdk_sts::Client::new(config);
        let output = sts
            .decode_authorization_message()
            .encoded_message(self.0.to_owned())
            .send()
            .await
            .map_err(EncodedAuthorizationMessageDecodeError::from_sdk)?;
        let message = output
            .decoded_message
            .expect("decode authorization message response without decoded_message");
        Ok(serde_json::from_str(&message).expect("decoded authorization message isn't JSON"))
    }
}

/// The error returned by [`EncodedAuthorizationMessage::decode`].
#[derive(Debug)]
pub struct EncodedAuthorizationMessageDecodeError(Box<dyn std::error::Error>);

impl EncodedAuthorizationMessageDecodeError {
    fn from_sdk(error: SdkError<DecodeAuthorizationMessageError>) -> Self {
        Self(error.into())
    }
}

impl fmt::Display for EncodedAuthorizationMessageDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EncodedAuthorizationMessageDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

/// A stack operation failed due to resource errors.
#[derive(Clone, Debug, Eq)]
pub struct ResourceErrors<'a> {
    logical_resource_ids: &'a str,
}

impl<'a> ResourceErrors<'a> {
    /// The logical resource IDs of resources that failed.
    pub fn logical_resource_ids(&self) -> impl Iterator<Item = &'a str> {
        lazy_static! {
            static ref LOGICAL_RESOURCE_ID: Regex = Regex::new("(?i)[a-z0-9]+").unwrap();
        }

        LOGICAL_RESOURCE_ID
            .find_iter(self.logical_resource_ids)
            .map(|m| m.as_str())
    }
}

/// Equality is implemented explicitly over [`logical_resource_ids`](Self::logical_resource_ids),
/// rather than derived structurally.
impl PartialEq for ResourceErrors<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.logical_resource_ids().eq(other.logical_resource_ids())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reason_detail() {
        #![allow(clippy::shadow_unrelated)]

        let example = r"Resource creation cancelled";
        assert_eq!(
            StatusReasonDetail::new(example),
            Some(StatusReasonDetail::CreationCancelled)
        );

        let example = r"API: ec2:ModifyVpcAttribute You are not authorized to perform this operation. Encoded authorization failure message: g1-YvnBabE1x9q868e9rU4VX9gFjPpt31dEvX6uYDMWmdkou9pGLq85c3Wy4IAr3CwKrF8Jqu0aIkiy0TBM5SU22pSjE-gzZuP1dg5rvyhI1fl5DBB4DiDyRmZpOjovE2w0MMxuM4QFqf6zAtlbCtwdCYVxHwTpKrlkQAJEr40twnTPWe1_Vh-YRfprV9RBis8nReUcf87GV1oGFxjLujid4oOAinD-NmpIUR5VLCw2ycoOZihPR_unBC9stRioVeYiBg-Q1T5IU-J-xEQK092YuR-H4vqMm5Nwg4l1kN10t8pbFb_YopmILVfvh-ViLBbzE0cO6ZlvLvcMcB8crsbgLP10H05hPtHDIGUMwc_xM-y_9SUAcrVUfPKdM4JeMvNMLkFfuLcgMIjTivxG1y3DwligaBXrSwKVkkMB4XfswrU7nYT6PO0cIyD_v7vw5kPJP1EafEZGVMJrJJEwS43FVFkLCMIi6eSxyFTYRF4GUbkuXbTpfMxYdivdFdiofA6_JsC-AZXwcE3qXAHpJ3PrH6lYfWm8z0m8PATAQKTqlcEMIYNngNnmnqasBQ_anBj-C7BT4V_B67wOOhc_Vwheq6xKnsI7XfsTgzsmHdFZDVIBCrdw";
        assert_eq!(
            StatusReasonDetail::new(example),
            Some(StatusReasonDetail::MissingPermission(MissingPermission {
                permission: "ec2:ModifyVpcAttribute",
                principal: None,
                encoded_authorization_message: Some(EncodedAuthorizationMessage::new("g1-YvnBabE1x9q868e9rU4VX9gFjPpt31dEvX6uYDMWmdkou9pGLq85c3Wy4IAr3CwKrF8Jqu0aIkiy0TBM5SU22pSjE-gzZuP1dg5rvyhI1fl5DBB4DiDyRmZpOjovE2w0MMxuM4QFqf6zAtlbCtwdCYVxHwTpKrlkQAJEr40twnTPWe1_Vh-YRfprV9RBis8nReUcf87GV1oGFxjLujid4oOAinD-NmpIUR5VLCw2ycoOZihPR_unBC9stRioVeYiBg-Q1T5IU-J-xEQK092YuR-H4vqMm5Nwg4l1kN10t8pbFb_YopmILVfvh-ViLBbzE0cO6ZlvLvcMcB8crsbgLP10H05hPtHDIGUMwc_xM-y_9SUAcrVUfPKdM4JeMvNMLkFfuLcgMIjTivxG1y3DwligaBXrSwKVkkMB4XfswrU7nYT6PO0cIyD_v7vw5kPJP1EafEZGVMJrJJEwS43FVFkLCMIi6eSxyFTYRF4GUbkuXbTpfMxYdivdFdiofA6_JsC-AZXwcE3qXAHpJ3PrH6lYfWm8z0m8PATAQKTqlcEMIYNngNnmnqasBQ_anBj-C7BT4V_B67wOOhc_Vwheq6xKnsI7XfsTgzsmHdFZDVIBCrdw")),
            }))
        );

        let example = r"API: s3:CreateBucket Access Denied";
        assert_eq!(
            StatusReasonDetail::new(example),
            Some(StatusReasonDetail::MissingPermission(MissingPermission {
                permission: "s3:CreateBucket",
                principal: None,
                encoded_authorization_message: None,
            }))
        );

        let example = r#"Resource handler returned message: "User: arn:aws:iam::012345678910:user/cloudformatious-cli-testing is not authorized to perform: elasticfilesystem:CreateFileSystem on the specified resource (Service: Efs, Status Code: 403, Request ID: fedb2f85-ff52-496c-b7be-207a23072587, Extended Request ID: null)" (RequestToken: ccd41719-eae9-3614-3b35-1d1cc3ad55da, HandlerErrorCode: GeneralServiceException)"#;
        assert_eq!(
            StatusReasonDetail::new(example),
            Some(StatusReasonDetail::MissingPermission(MissingPermission {
                permission: "elasticfilesystem:CreateFileSystem",
                principal: Some("arn:aws:iam::012345678910:user/cloudformatious-cli-testing"),
                encoded_authorization_message: None,
            }))
        );

        let example =
            r"The following resource(s) failed to create: [Vpc, Fs]. Rollback requested by user.";
        let detail = StatusReasonDetail::new(example).unwrap();
        assert_eq!(
            detail,
            StatusReasonDetail::ResourceErrors(ResourceErrors {
                logical_resource_ids: "Vpc, Fs",
            })
        );
        if let StatusReasonDetail::ResourceErrors(resource_errors) = detail {
            assert_eq!(
                resource_errors.logical_resource_ids().collect::<Vec<_>>(),
                vec!["Vpc", "Fs"]
            );
        } else {
            unreachable!()
        }
    }
}
