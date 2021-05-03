use std::{convert::TryInto, env, process};

use cloudformatious::{ApplyStackInput, CloudFormatious, DeleteStackInput, TemplateSource};
use futures_util::StreamExt;
use rusoto_cloudformation::CloudFormationClient;
use rusoto_core::Region;

const USAGE: &str = "Usage: cargo run --example cli -- <apply|delete> <stack_name> [template_body]";

enum Op {
    Apply,
    Delete,
}

impl std::str::FromStr for Op {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "apply" => Ok(Self::Apply),
            "delete" => Ok(Self::Delete),
            _ => Err(()),
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = try_main().await {
        eprintln!("{}", error);
        process::exit(1);
    }
}

async fn try_main() -> Result<(), Box<dyn std::error::Error>> {
    let [op, stack_name]: [_; 2] = env::args()
        .skip(1)
        .take(2)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| USAGE)?;
    let op = op.parse().map_err(|()| USAGE)?;

    let client = CloudFormationClient::new(Region::default());

    match op {
        Op::Apply => {
            let template_body = env::args().nth(3).ok_or(USAGE)?;

            let input = ApplyStackInput::new(stack_name, TemplateSource::inline(template_body));
            let mut apply = client.apply_stack(input);

            let mut events = apply.events();
            while let Some(event) = events.next().await {
                eprintln!("{:?}", event);
            }

            let output = apply.await?;
            eprintln!("=== Operation succeeded ===");
            eprintln!("{:#?}", output);
            eprintln!();
        }
        Op::Delete => {
            if env::args().nth(3).is_some() {
                return Err(USAGE.into());
            }

            let input = DeleteStackInput::new(stack_name);
            let mut delete = client.delete_stack(input);

            let mut events = delete.events();
            while let Some(event) = events.next().await {
                eprintln!("{:?}", event);
            }

            delete.await?;
            eprintln!("=== Operation succeeded ===");
        }
    }

    Ok(())
}
