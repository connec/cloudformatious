use aws_sdk_cloudformation::types::StackStatus;
use futures_util::StreamExt;

use cloudformatious::{ApplyStackInput, DeleteStackInput, Parameter, TemplateSource};

use crate::common::{generated_name, get_client, EMPTY_TEMPLATE, NON_EMPTY_TEMPLATE};

#[tokio::test]
async fn delete_stack_fut_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
    let stack = client.apply_stack(input).await?;

    let input = DeleteStackInput::new(&stack_name);
    client.delete_stack(input).await?;

    let stack = {
        let config = aws_config::load_from_env().await;
        let client = aws_sdk_cloudformation::Client::new(&config);
        client
            .describe_stacks()
            .stack_name(stack.stack_id)
            .send()
            .await?
            .stacks
            .unwrap()
            .pop()
            .unwrap()
    };

    assert_eq!(stack.stack_status.unwrap(), StackStatus::DeleteComplete);

    Ok(())
}

#[tokio::test]
async fn delete_stack_stream_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(NON_EMPTY_TEMPLATE))
        .set_parameters([Parameter {
            key: "CidrBlock".to_string(),
            value: "10.0.0.0/28".to_string(),
        }]);
    let stack = client.apply_stack(input).await?;

    let input = DeleteStackInput::new(&stack_name);
    let mut delete = client.delete_stack(input);

    let events: Vec<_> = delete
        .events()
        .map(|event| {
            (
                event.logical_resource_id().to_string(),
                event.resource_status().to_string(),
            )
        })
        .collect()
        .await;
    delete.await?;

    assert_eq!(
        events,
        vec![
            (stack_name.clone(), "DELETE_IN_PROGRESS".to_string()),
            ("Subnet".to_string(), "DELETE_IN_PROGRESS".to_string()),
            ("Subnet".to_string(), "DELETE_COMPLETE".to_string()),
            (stack_name.clone(), "DELETE_COMPLETE".to_string())
        ]
    );

    let stack = {
        let config = aws_config::load_from_env().await;
        let client = aws_sdk_cloudformation::Client::new(&config);
        client
            .describe_stacks()
            .stack_name(stack.stack_id)
            .send()
            .await?
            .stacks
            .unwrap()
            .pop()
            .unwrap()
    };

    assert_eq!(stack.stack_status.unwrap(), StackStatus::DeleteComplete);

    Ok(())
}

#[tokio::test]
async fn delete_stack_fut_noop() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

    let stack_name = generated_name();
    let input = DeleteStackInput::new(&stack_name);
    client.delete_stack(input).await?;

    Ok(())
}

#[tokio::test]
async fn delete_stack_stream_noop() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

    let stack_name = generated_name();
    let input = DeleteStackInput::new(&stack_name);
    let mut delete = client.delete_stack(input);

    let events: Vec<_> = delete.events().collect().await;
    delete.await?;

    assert_eq!(events, vec![]);

    Ok(())
}

#[tokio::test]
async fn delete_stack_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
    let stack = client.apply_stack(input).await?;

    let input = DeleteStackInput::new(&stack.stack_id);
    client.delete_stack(input).await?;

    let input = DeleteStackInput::new(&stack.stack_id);
    client.delete_stack(input).await?;

    Ok(())
}
