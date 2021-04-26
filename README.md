# `cloudformatious`

[![crates.io](https://img.shields.io/crates/v/cloudformatious?logo=rust&style=flat-square)](https://crates.io/crates/cloudformatious)
[![docs.rs](https://img.shields.io/docsrs/cloudformatious?logo=rust&style=flat-square)](https://docs.rs/cloudformatious)

⚠️ This crate is WIP.

An extension trait for [`rusoto_cloudformation::CloudFormationClient`](https://docs.rs/rusoto_cloudformation/0.46.0/rusoto_cloudformation/struct.CloudFormationClient.html) offering richly typed higher-level APIs to perform long-running operations and await their termination or observe their progress.

```rust + no_run
use futures_util::TryStreamExt;
use rusoto_cloudformation::CloudFormationClient;
use rusoto_core::Region;

use cloudformatious::{ApplyEvent, ApplyInput, CloudFormatious, TemplateSource};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CloudFormationClient::new(Region::EuWest2);

    let input = ApplyInput::new("my-stack", TemplateSource::inline("{}"));
    let mut apply = client.apply(input);
    let mut output = None; // A cleaner way of getting the output is also on the list...

    while let Some(event) = apply.try_next().await? {
        match event {
            ApplyEvent::Event(event) => eprintln!("{:#?}", event),
            ApplyEvent::Output(output_) => output = Some(output_),
        }
    };

    eprintln!("Apply success!");
    println!("{:#?}", output.unwrap());

    Ok(())
}
```

## Motivation

CloudFormation's API is relatively low-level.
This makes it possible to implement fairly advanced workflows involving things like manual review of changes before applying, but it makes the common case of an idempotent 'apply this template' deployment a bit awkward.
There are other tools that can mitigate this, such as the `aws cloudformation deploy` high-level command, but their output is very limited so they only really do half of the work.
Furthermore, the tools that I'm aware of are primarily invoked from the shell, meaning they cannot be integrated natively into programs that wish to orchestrate CloudFormation stacks.

Also, I like CloudFormation and programming in Rust so this is fun for me 🤷‍♂️

## Current status

There is a `CloudFormatious` extension trait with an `apply` method, which implements an idempotent 'update or create stack' operation.
It's roughly equivalent to the [`aws cloudformation deploy`](https://docs.aws.amazon.com/cli/latest/reference/cloudformation/deploy/index.html) command, but with better programmatic access to inputs, events, and outputs.

I will probably want a similar '`Future` or `Stream`' API for stack deletion, but I have no needs beyond that for my current use cases.

## Contributing

Feedback and PRs are welcome.
However, if you'd like to add any non-trivial functionality it may be worth opening an issue to discuss it first.

## License

[MIT](https://choosealicense.com/licenses/mit/)
