use std::{env, time::Duration};

use rusoto_cloudformation::{CloudFormation, CloudFormationClient, DeleteStackInput};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};

use rusoto_cloudformation_ext::{
    ApplyError, ApplyInput, CloudFormationExt, ResourceStatus, StackStatus,
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
async fn create_stack_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput {
        capabilities: Vec::new(),
        client_request_token: None,
        notification_arns: Vec::new(),
        parameters: Vec::new(),
        resource_types: None,
        role_arn: None,
        stack_name: stack_name.clone(),
        tags: Vec::new(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        template_url: None,
    };
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
async fn idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput {
        capabilities: Vec::new(),
        client_request_token: None,
        notification_arns: Vec::new(),
        parameters: Vec::new(),
        resource_types: None,
        role_arn: None,
        stack_name: stack_name.clone(),
        tags: Vec::new(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        template_url: None,
    };
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
async fn create_stack_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput {
        capabilities: Vec::new(),
        client_request_token: None,
        notification_arns: Vec::new(),
        parameters: Vec::new(),
        resource_types: None,
        role_arn: None,
        stack_name: stack_name.clone(),
        tags: Vec::new(),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        template_url: None,
    };
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
            resource_events.get(0).map(|(status, details)| {
                (
                    details.logical_resource_id(),
                    *status,
                    details.resource_status_reason(),
                )
            }),
            Some((
                "Vpc",
                ResourceStatus::CreateFailed,
                Some("Property CidrBlock cannot be empty.")
            ))
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
async fn create_change_set_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput {
        capabilities: Vec::new(),
        client_request_token: None,
        notification_arns: Vec::new(),
        parameters: Vec::new(),
        resource_types: None,
        role_arn: None,
        stack_name: stack_name.clone(),
        tags: Vec::new(),
        template_body: Some("".to_string()),
        template_url: None,
    };
    let error = client.apply(input).await.unwrap_err();
    if let ApplyError::CloudFormationApi { .. } = error {
    } else {
        return Err(error.into());
    }

    Ok(())
}

#[tokio::test]
async fn update_stack_err() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyInput {
        capabilities: Vec::new(),
        client_request_token: None,
        notification_arns: Vec::new(),
        parameters: Vec::new(),
        resource_types: None,
        role_arn: None,
        stack_name: stack_name.clone(),
        tags: Vec::new(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        template_url: None,
    };
    let output = client.apply(input).await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    let input = ApplyInput {
        capabilities: Vec::new(),
        client_request_token: None,
        notification_arns: Vec::new(),
        parameters: Vec::new(),
        resource_types: None,
        role_arn: None,
        stack_name: stack_name.clone(),
        tags: Vec::new(),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        template_url: None,
    };
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
            resource_events.get(0).map(|(status, details)| {
                (
                    details.logical_resource_id(),
                    *status,
                    details.resource_status_reason(),
                )
            }),
            Some((
                "Vpc",
                ResourceStatus::CreateFailed,
                Some("Property CidrBlock cannot be empty.")
            ))
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
