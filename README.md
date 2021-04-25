# `cloudformatious`

⚠️ This crate is WIP.

Extension traits for [`rusoto_cloudformation::CloudFormationClient`](https://docs.rs/rusoto_cloudformation/0.46.0/rusoto_cloudformation/struct.CloudFormationClient.html) offering higher-level APIs to perform long-running operations and await their termination or observe their progress.

## Motivation

CloudFormation's API is relatively low-level.
This makes it possible to implement fairly advanced workflows involving things like manual review of changes before applying, but it makes the common case of an idempotent 'apply this template' deployment a bit awkward.
There are other tools that can mitigate this, such as the `aws cloudformation deploy` high-level command, but their output is very limited so they only really do half of the work.
Furthermore, the tools that I'm aware of are primarily invoked from the shell, meaning they cannot be integrated natively into programs that wish to orchestrate CloudFormation stacks.

Also, I like CloudFormation and programming in Rust so this is fun for me 🤷‍♂️

## Goal

The initial goal of this library is to support an idempotent deployment workflow whereby a CloudFormation template is 'applied' to an AWS environment (account and region):

- If there is no stack in the AWS environment, one should be created; if one exists it should be updated.
- If there are no changes, the operation should succeed.
- The progress of the operation should be observeable.
- The stack outputs should be returned in a successful result.
- The operation should fail if the stack operation fails.

This can all be achieved by orchestrating CloudFormation APIs, but this library should offer a single API that offers this behaviour.
