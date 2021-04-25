# rusoto-cloudformation-ext

## Integration tests

Run these tests with `cargo run` from the project root.

### Requirements

The integration tests perform actual CloudFormation operations against an actual AWS account, and as such need actual AWS credentials.
Any mechanism supported by [`rusoto_credential::ChainProvider`](https://docs.rs/rusoto_credential/0.46.0/rusoto_credential/struct.ChainProvider.html) will work.

The identity to which the credentials pertain will need the following IAM policy to successfully run the tests:

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "CloudFormationStackOperations",
            "Effect": "Allow",
            "Action": [
                "cloudformation:CreateChangeSet",
                "cloudformation:DeleteStack",
                "cloudformation:DescribeChangeSet",
                "cloudformation:DescribeStackEvents",
                "cloudformation:DescribeStacks",
                "cloudformation:ExecuteChangeSet"
            ],
            "Resource": "arn:aws:cloudformation:*:*:stack/rusoto-cloudformation-ext-testing-*"
        }
    ]
}
```
