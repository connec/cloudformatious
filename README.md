# `rusoto_cloudformation_ext`

Extension traits for [`rusoto_cloudformation::CloudFormationClient`](https://docs.rs/rusoto_cloudformation/0.46.0/rusoto_cloudformation/struct.CloudFormationClient.html) offering higher-level APIs to perform long-running operations and await their termination or observe their progress.

## Current status

- [x] Raw `*_stream` APIs that start an operation and return a `Stream` of events.
- [x] Some simple integration tests.
- [ ] 'Cooked' variants of the above that tighten up the `rusoto_cloudformation` types to be more misuse-resistant.
- [ ] Make repository public and enable tests on PR as required status checks.
- [ ] Publish to crates.io.
- [ ] Mad unification of error types with some kind of `Operation` trait.
- [ ] Mad unification of logic with some kind of `Operation` trait.
