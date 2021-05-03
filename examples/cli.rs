use std::{convert::TryInto, env, io, process};

use cloudformatious::{ApplyStackInput, CloudFormatious, DeleteStackInput, TemplateSource};
use futures_util::StreamExt;
use rusoto_cloudformation::CloudFormationClient;
use rusoto_core::Region;

const USAGE: &str = "Usage: cargo run --example cli -- <apply|delete> <stack_name> [template_body]";

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

    let client = CloudFormationClient::new(Region::default());

    match op.as_str() {
        "apply" => {
            let template_body = env::args().nth(3).ok_or(USAGE)?;

            let input = ApplyStackInput::new(stack_name, TemplateSource::inline(template_body));
            let mut apply = client.apply_stack(input);

            let change_set = apply.change_set().await?;
            eprintln!("=== Change set ===");
            eprintln!("{:#?}", change_set);
            eprintln!();

            confirm("Continue [Y/n]?").await?;
            eprintln!();

            let mut events = apply.events();
            while let Some(event) = events.next().await {
                eprintln!("{:?}", event);
            }
            eprintln!();

            let output = apply.await?;
            eprintln!("=== Operation succeeded ===");
            eprintln!("{:#?}", output);
            eprintln!();
        }
        "delete" => {
            if env::args().nth(3).is_some() {
                return Err(USAGE.into());
            }

            let input = DeleteStackInput::new(stack_name);
            let mut delete = client.delete_stack(input);

            let mut events = delete.events();
            while let Some(event) = events.next().await {
                eprintln!("{:?}", event);
            }
            eprintln!();

            delete.await?;
            eprintln!("=== Operation succeeded ===");
        }
        _ => return Err(USAGE.into()),
    }

    Ok(())
}

async fn confirm(prompt: &str) -> io::Result<()> {
    eprint!("{} ", prompt);

    tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.as_str().trim() {
            "Y" | "y" | "" => Ok(()),
            _ => Err(io::Error::new(io::ErrorKind::Other, "Quitting")),
        }
    })
    .await
    .unwrap()
}
