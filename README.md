# `cloudformatious`

[![crates.io](https://img.shields.io/crates/v/cloudformatious?logo=rust&style=flat-square)](https://crates.io/crates/cloudformatious)
[![docs.rs](https://img.shields.io/docsrs/cloudformatious?logo=rust&style=flat-square)](https://docs.rs/cloudformatious)

A CloudFormation library offering richly typed higher-level APIs to perform long-running operations and await their termination or observe their progress.

```rust,no_run
use futures_util::StreamExt;

use cloudformatious::{ApplyStackInput, DeleteStackInput, TemplateSource};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = cloudformatious::Client::new(&config);

    let input = ApplyStackInput::new("my-stack", TemplateSource::inline("{}"));
    let mut stack = client.apply_stack(input);

    let mut events = stack.events();
    while let Some(event) = events.next().await {
        eprintln!("{:#?}", event);
    };

    let output = stack.await?;
    eprintln!("Stack applied");
    println!("{:#?}", output);

    let input = DeleteStackInput::new(output.stack_id);
    client.delete_stack(input).await?;

    println!("Stack deleted");

    Ok(())
}
```

## Motivation

CloudFormation's API is relatively low-level.
This makes it possible to implement fairly advanced workflows involving things like manual review of changes before applying, but it makes the common case of an idempotent 'apply this template' deployment a bit awkward.
There are other tools that can mitigate this, such as the `aws cloudformation deploy` high-level command, but their output is very limited so they only really do half of the work.
Furthermore, the tools that I'm aware of are primarily invoked from the shell, meaning they cannot be integrated natively into programs that wish to orchestrate CloudFormation stacks.

## Features

The `CloudFormatious` extension trait has the following methods:

- [`apply_stack`] which implements an idempotent 'update or create stack' operation.
- [`delete_stack`] which implements an idempotent delete stack operation.

[`apply_stack`]: https://docs.rs/cloudformatious/latest/cloudformatious/trait.CloudFormatious.html#method.apply_stack
[`delete_stack`]: https://docs.rs/cloudformatious/latest/cloudformatious/trait.CloudFormatious.html#method.delete_stack

In both cases, the API is a bit more ergonomic than `aws_sdk_cloudformation` and the API is richer.
In particular:

- The return value of both methods implements `Future`, which can be `await`ed to wait for the overall operation to end.
- The return value of both methods has an `events()` method, which can be used to get a `Stream` of stack events that occur during the operation.
- Both methods return rich `Err` values if the stack settles in a failing state.
- Both methods return rich `Err` values if the stack operation succeeds, but some resource(s) had errors (these "warnings" can be ignored, but it may mean leaving extraneous infrastructure in your environment).
- `apply_stack` returns a rich `Ok` value with 'cleaner' types than the generated `aws_sdk_cloudformation` types (fewer redundant `Option`s, `enum`s for mutually exclusive states, etc.).

## Contributing

Feedback and PRs are welcome.
However, if you'd like to add any non-trivial functionality it may be worth opening an issue to discuss it first.

## License

[MIT](https://choosealicense.com/licenses/mit/)
