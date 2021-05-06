use std::{env, time::Duration};

use futures_util::StreamExt;
use rusoto_cloudformation::{CloudFormationClient, DescribeStacksInput};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};

use cloudformatious::{
    change_set::{Action, ExecutionStatus},
    ApplyStackError, ApplyStackInput, ChangeSetStatus, CloudFormatious, DeleteStackInput,
    ResourceStatus, StackFailure, StackStatus, TemplateSource,
};

const NAME_PREFIX: &str = "rusoto-cloudformation-ext-testing-";
const DUMMY_TEMPLATE: &str = r#"{
    "Conditions": {
        "Never": { "Fn::Equals": [true, false] }
    },
    "Resources": {
        "Fake": {
            "Type": "Custom::Fake",
            "Condition": Never
        }
    }
}"#;
const NON_EMPTY_TEMPLATE: &str = r#"{
    "Resources": {
        "Dummy": {
            "Type": "AWS::CloudFormation::WaitConditionHandle",
            "Properties": {}
        }
    }
}"#;
const FAILING_TEMPLATE: &str = r#"
            {
                "Resources": {
                    "Vpc": {
                        "Type": "AWS::EC2::VPC",
                        "Properties": {}
                    }
                }
            }
            "#;

#[tokio::test]
async fn create_stack_fut_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let output = client.apply_stack(input).await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_change_set_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let mut stack = client.apply_stack(input);

    let change_set = stack.change_set().await?;
    assert_eq!(change_set.status, ChangeSetStatus::CreateComplete);
    assert_eq!(change_set.execution_status, ExecutionStatus::Available);
    assert!(change_set.changes.is_empty());

    let output = stack.await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_change_set_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let mut stack = client.apply_stack(input);

    let change_set = stack.change_set().await?;
    assert_eq!(change_set.status, ChangeSetStatus::CreateComplete);
    assert_eq!(change_set.execution_status, ExecutionStatus::Available);

    let changes: Vec<_> = change_set
        .changes
        .iter()
        .map(|change| {
            (
                change.action,
                change.logical_resource_id.as_str(),
                change.resource_type.as_str(),
            )
        })
        .collect();
    assert_eq!(changes, vec![(Action::Add, "Vpc", "AWS::EC2::VPC")]);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_stream_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let mut stack = client.apply_stack(input);

    let events: Vec<_> = stack
        .events()
        .map(|event| {
            (
                event.logical_resource_id().to_string(),
                event.resource_status().to_string(),
            )
        })
        .collect()
        .await;
    let output = stack.await?;

    assert_eq!(output.stack_status, StackStatus::CreateComplete);
    assert_eq!(
        events,
        vec![
            (stack_name.clone(), "CREATE_IN_PROGRESS".to_string()),
            (stack_name.clone(), "CREATE_COMPLETE".to_string()),
        ]
    );

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_change_set_and_stream_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let mut stack = client.apply_stack(input);

    let change_set = stack.change_set().await?;
    assert_eq!(change_set.status, ChangeSetStatus::CreateComplete);
    assert_eq!(change_set.execution_status, ExecutionStatus::Available);
    assert!(change_set.changes.is_empty());

    let events: Vec<_> = stack
        .events()
        .map(|event| {
            (
                event.logical_resource_id().to_string(),
                event.resource_status().to_string(),
            )
        })
        .collect()
        .await;
    let output = stack.await?;

    assert_eq!(output.stack_status, StackStatus::CreateComplete);
    assert_eq!(
        events,
        vec![
            (stack_name.clone(), "CREATE_IN_PROGRESS".to_string()),
            (stack_name.clone(), "CREATE_COMPLETE".to_string()),
        ]
    );

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn apply_overall_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));

    let mut apply = client.apply_stack(input.clone());
    let change_set = apply.change_set().await?;
    assert_eq!(change_set.status, ChangeSetStatus::CreateComplete);
    assert_eq!(change_set.execution_status, ExecutionStatus::Available);
    assert!(change_set.changes.is_empty());
    let output1 = apply.await?;

    let mut apply = client.apply_stack(input);
    let change_set = apply.change_set().await?;
    assert_eq!(change_set.status, ChangeSetStatus::Failed);
    assert!(change_set.status_reason.is_some());
    assert_eq!(change_set.execution_status, ExecutionStatus::Unavailable);
    assert!(change_set.changes.is_empty());
    let output2 = apply.await?;

    assert_eq!(output2.stack_status, StackStatus::CreateComplete);
    assert_eq!(output1, output2);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_fut_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let error = client.apply_stack(input).await.unwrap_err();
    if let ApplyStackError::Failure(StackFailure {
        stack_status,
        stack_status_reason,
        resource_events,
        ..
    }) = error
    {
        assert_eq!(stack_status, StackStatus::RollbackComplete);
        assert!(stack_status_reason.contains("resource(s) failed to create: [Vpc]"));
        assert_eq!(
            resource_events
                .iter()
                .map(|(status, details)| {
                    (
                        details.logical_resource_id(),
                        *status,
                        details.resource_status_reason(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![(
                "Vpc",
                ResourceStatus::CreateFailed,
                Some("Property CidrBlock cannot be empty.")
            )]
        );
    } else {
        return Err(error.into());
    }

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_stream_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let mut stack = client.apply_stack(input);

    let events: Vec<_> = stack
        .events()
        .map(|event| {
            (
                event.logical_resource_id().to_string(),
                event.resource_status().to_string(),
            )
        })
        .collect()
        .await;
    let error = stack.await.unwrap_err();

    assert_eq!(
        events,
        vec![
            (stack_name.clone(), "CREATE_IN_PROGRESS".to_string()),
            ("Vpc".to_string(), "CREATE_FAILED".to_string()),
            (stack_name.clone(), "ROLLBACK_IN_PROGRESS".to_string()),
            ("Vpc".to_string(), "DELETE_COMPLETE".to_string()),
            (stack_name.clone(), "ROLLBACK_COMPLETE".to_string()),
        ]
    );
    if let ApplyStackError::Failure(StackFailure {
        stack_status,
        stack_status_reason,
        resource_events,
        ..
    }) = error
    {
        assert_eq!(stack_status, StackStatus::RollbackComplete);
        assert!(stack_status_reason.contains("resource(s) failed to create: [Vpc]"));
        assert_eq!(
            resource_events
                .iter()
                .map(|(status, details)| {
                    (
                        details.logical_resource_id(),
                        *status,
                        details.resource_status_reason(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![(
                "Vpc",
                ResourceStatus::CreateFailed,
                Some("Property CidrBlock cannot be empty.")
            )]
        );
    } else {
        return Err(error.into());
    }

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_change_set_fut_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(""));
    let error = client.apply_stack(input).await.unwrap_err();
    if let ApplyStackError::CloudFormationApi { .. } = error {
    } else {
        return Err(error.into());
    }

    Ok(())
}

#[tokio::test]
async fn update_stack_fut_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let output = client.apply_stack(input).await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let error = client.apply_stack(input).await.unwrap_err();
    if let ApplyStackError::Failure(StackFailure {
        stack_status,
        stack_status_reason,
        resource_events,
        ..
    }) = error
    {
        assert_eq!(stack_status, StackStatus::UpdateRollbackComplete);
        assert!(stack_status_reason.contains("resource(s) failed to create: [Vpc]"));
        assert_eq!(
            resource_events
                .iter()
                .map(|(status, details)| {
                    (
                        details.logical_resource_id(),
                        *status,
                        details.resource_status_reason(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![(
                "Vpc",
                ResourceStatus::CreateFailed,
                Some("Property CidrBlock cannot be empty.")
            )]
        );
    } else {
        return Err(error.into());
    }

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn delete_stack_fut_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let stack = client.apply_stack(input).await?;

    let input = DeleteStackInput::new(&stack_name);
    client.delete_stack(input).await?;

    let input = DescribeStacksInput {
        stack_name: Some(stack.stack_id),
        ..DescribeStacksInput::default()
    };
    let stack = {
        use rusoto_cloudformation::CloudFormation;
        client
            .describe_stacks(input)
            .await?
            .stacks
            .unwrap()
            .pop()
            .unwrap()
    };

    assert_eq!(stack.stack_status, "DELETE_COMPLETE");

    Ok(())
}

#[tokio::test]
async fn delete_stack_stream_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(NON_EMPTY_TEMPLATE));
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
            ("Dummy".to_string(), "DELETE_IN_PROGRESS".to_string()),
            ("Dummy".to_string(), "DELETE_COMPLETE".to_string()),
            (stack_name.clone(), "DELETE_COMPLETE".to_string())
        ]
    );

    let input = DescribeStacksInput {
        stack_name: Some(stack.stack_id),
        ..DescribeStacksInput::default()
    };
    let stack = {
        use rusoto_cloudformation::CloudFormation;
        client
            .describe_stacks(input)
            .await?
            .stacks
            .unwrap()
            .pop()
            .unwrap()
    };

    assert_eq!(stack.stack_status, "DELETE_COMPLETE");

    Ok(())
}

#[tokio::test]
async fn delete_stack_fut_noop() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = DeleteStackInput::new(&stack_name);
    client.delete_stack(input).await?;

    Ok(())
}

#[tokio::test]
async fn delete_stack_stream_noop() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

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
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let stack = client.apply_stack(input).await?;

    let input = DeleteStackInput::new(&stack.stack_id);
    client.delete_stack(input).await?;

    let input = DeleteStackInput::new(&stack.stack_id);
    client.delete_stack(input).await?;

    Ok(())
}

fn get_client() -> CloudFormationClient {
    let client = HttpClient::new().unwrap();

    let mut credentials = AutoRefreshingProvider::new(ChainProvider::new()).unwrap();
    credentials.get_mut().set_timeout(Duration::from_secs(1));

    let region = env::var("AWS_REGION").expect("You must set AWS_REGION to run these tests");
    let region = region.parse().expect("Invalid AWS region");

    CloudFormationClient::new_with(client, credentials, region)
}

fn generated_name() -> String {
    format!("{}{}", NAME_PREFIX, fastrand::u32(..))
}

async fn clean_up(
    client: &CloudFormationClient,
    stack_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    use rusoto_cloudformation::{CloudFormation, DeleteStackInput};
    CloudFormation::delete_stack(
        client,
        DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        },
    )
    .await
    .map_err(Into::into)
}
