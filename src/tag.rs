/// A resource tag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tag {
    /// The tag key.
    pub key: String,

    /// The tag value.
    pub value: String,
}

impl Tag {
    pub(crate) fn from_sdk(tag: aws_sdk_cloudformation::types::Tag) -> Self {
        Self {
            key: tag.key.expect("Tag without key"),
            value: tag.value.expect("Tag without value"),
        }
    }

    pub(crate) fn into_sdk(self) -> aws_sdk_cloudformation::types::Tag {
        aws_sdk_cloudformation::types::Tag::builder()
            .key(self.key)
            .value(self.value)
            .build()
    }
}
