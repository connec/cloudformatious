# `rusoto_cloudformation_ext`

Extension traits for [`rusoto_cloudformation::CloudFormationClient`](https://docs.rs/rusoto_cloudformation/0.46.0/rusoto_cloudformation/struct.CloudFormationClient.html) offering higher-level APIs to perform long-running operations and await their termination or observe their progress.

## Current status

- [x] `*_checked` APIs that start an operation and poll until it completes.
- [x] Some simple integration tests.
- [ ] `*_stream` APIs that start an operation and return a `Stream` of events.
- [ ] 'Cooked' variants of the above that tighten up the `rusoto_cloudformation` types to be more misuse-resistant.
- [ ] Mad unification of error types with some kind of `Operation` trait.
- [ ] Mad unification of logic with some kind of `Operation` trait.
