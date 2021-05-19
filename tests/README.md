# `cloudformatious` tests

Run these tests with `cargo run` from the project root.

## Requirements

The integration tests perform actual CloudFormation operations against an actual AWS account, and as such need actual AWS credentials.
Any mechanism supported by [`rusoto_credential::ChainProvider`](https://docs.rs/rusoto_credential/0.46.0/rusoto_credential/struct.ChainProvider.html) will work.

A CloudFormation template is included to deploy test dependencies: [`cloudformatious-testing.yaml`](cloudformatious-testing.yaml).
Currently this is just an IAM policy granting CloudFormation permissions.
You will need to attach the generated policy to principal you use to run the tests.
