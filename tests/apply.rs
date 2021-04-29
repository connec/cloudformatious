use std::{env, time::Duration};

use futures_util::StreamExt;
use rusoto_cloudformation::{CloudFormation, CloudFormationClient, DeleteStackInput};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};

use cloudformatious::{
    ApplyError, ApplyInput, CloudFormatious, ResourceStatus, StackStatus, TemplateSource,
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
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let output = client.apply(input).await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_stream_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let mut apply = client.apply(input);

    let events: Vec<_> = (&mut apply)
        .map(|event| {
            (
                event.logical_resource_id().to_string(),
                event.resource_status().to_string(),
            )
        })
        .collect()
        .await;
    let output = apply.await?;

    assert_eq!(output.stack_status, StackStatus::CreateComplete);
    assert_eq!(
        events,
        vec![
            (stack_name.clone(), "CREATE_IN_PROGRESS".to_string()),
            (stack_name.clone(), "CREATE_COMPLETE".to_string()),
        ]
    );

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let output1 = client.apply(input.clone()).await?;
    let output2 = client.apply(input).await?;
    assert_eq!(output2.stack_status, StackStatus::CreateComplete);
    assert_eq!(output1, output2);

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_fut_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let error = client.apply(input).await.unwrap_err();
    if let ApplyError::Failure {
        stack_status,
        stack_status_reason,
        resource_events,
        ..
    } = error
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

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_stream_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let mut apply = client.apply(input);

    let events: Vec<_> = (&mut apply)
        .map(|event| {
            (
                event.logical_resource_id().to_string(),
                event.resource_status().to_string(),
            )
        })
        .collect()
        .await;
    let error = apply.await.unwrap_err();

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
    if let ApplyError::Failure {
        stack_status,
        stack_status_reason,
        resource_events,
        ..
    } = error
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

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn create_change_set_fut_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(""));
    let error = client.apply(input).await.unwrap_err();
    if let ApplyError::CloudFormationApi { .. } = error {
    } else {
        return Err(error.into());
    }

    Ok(())
}

#[tokio::test]
async fn update_stack_fut_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput::new(&stack_name, TemplateSource::inline(DUMMY_TEMPLATE));
    let output = client.apply(input).await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    let input = ApplyInput::new(&stack_name, TemplateSource::inline(FAILING_TEMPLATE));
    let error = client.apply(input).await.unwrap_err();
    if let ApplyError::Failure {
        stack_status,
        stack_status_reason,
        resource_events,
        ..
    } = error
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

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

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
