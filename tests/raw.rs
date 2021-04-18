use std::{env, error::Error, time::Duration};

use rusoto_cloudformation::{
    CloudFormation, CloudFormationClient, CreateChangeSetInput, DeleteStackInput,
    ExecuteChangeSetInput,
};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};
use tokio_stream::StreamExt;

use rusoto_cloudformation_ext::raw::{CloudFormationExt, CreateChangeSetWaitError};

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
async fn create_change_set_wait() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Successful create
    let create_change_set_input = CreateChangeSetInput {
        stack_name: generated_name(),
        change_set_name: generated_name(),
        change_set_type: Some("CREATE".to_string()),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateChangeSetInput::default()
    };
    let change_set = client
        .create_change_set_wait(create_change_set_input)
        .await?;
    assert_eq!(change_set.execution_status.as_deref(), Some("AVAILABLE"));
    assert_eq!(change_set.status.as_deref(), Some("CREATE_COMPLETE"));

    // Failed create
    let execute_change_set_input = ExecuteChangeSetInput {
        change_set_name: change_set.change_set_id.unwrap(),
        ..ExecuteChangeSetInput::default()
    };
    client
        .execute_change_set_stream(execute_change_set_input)
        .await?
        .collect::<Result<Vec<_>, _>>()
        .await?;

    let create_change_set_input = CreateChangeSetInput {
        stack_name: change_set.stack_id.unwrap(),
        change_set_name: generated_name(),
        change_set_type: Some("UPDATE".to_string()),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateChangeSetInput::default()
    };
    let change_set = client
        .create_change_set_wait(create_change_set_input)
        .await?;
    assert_eq!(change_set.execution_status.as_deref(), Some("UNAVAILABLE"));
    assert_eq!(change_set.status.as_deref(), Some("FAILED"));

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: change_set.stack_name.unwrap(),
            ..DeleteStackInput::default()
        })
        .await?;

    // CreateChangeSet error
    let create_change_set_input = CreateChangeSetInput::default();
    let change_set_result = client.create_change_set_wait(create_change_set_input).await;
    assert!(matches!(
        change_set_result,
        Err(CreateChangeSetWaitError::CreateChangeSet(_))
    ));

    Ok(())
}

#[tokio::test]
async fn execute_change_set_stream() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Create a change set to execute
    let stack_name = generated_name();
    let create_change_set_input = CreateChangeSetInput {
        stack_name: stack_name.clone(),
        change_set_name: generated_name(),
        change_set_type: Some("CREATE".to_string()),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateChangeSetInput::default()
    };
    let change_set = client
        .create_change_set_wait(create_change_set_input)
        .await?;

    // Successful execution
    let execute_change_set_input = ExecuteChangeSetInput {
        change_set_name: change_set.change_set_id.unwrap(),
        ..ExecuteChangeSetInput::default()
    };
    let stack_events: Vec<_> = client
        .execute_change_set_stream(execute_change_set_input)
        .await?
        .collect::<Result<_, _>>()
        .await?;
    let resource_statuses: Vec<_> = stack_events
        .iter()
        .map(|stack_event| {
            (
                stack_event.logical_resource_id.as_deref().unwrap(),
                stack_event.resource_status.as_deref().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        resource_statuses,
        vec![
            (stack_name.as_str(), "CREATE_IN_PROGRESS"),
            (stack_name.as_str(), "CREATE_COMPLETE")
        ]
    );

    // Create an update change set
    let create_change_set_input = CreateChangeSetInput {
        stack_name: stack_name.clone(),
        change_set_name: generated_name(),
        change_set_type: Some("UPDATE".to_string()),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        ..CreateChangeSetInput::default()
    };
    let change_set = client
        .create_change_set_wait(create_change_set_input)
        .await?;

    // Failed execution
    let execute_change_set_input = ExecuteChangeSetInput {
        change_set_name: change_set.change_set_id.unwrap(),
        ..ExecuteChangeSetInput::default()
    };
    let stack_events: Vec<_> = client
        .execute_change_set_stream(execute_change_set_input)
        .await?
        .collect::<Result<_, _>>()
        .await?;
    let resource_statuses: Vec<_> = stack_events
        .iter()
        .map(|stack_event| {
            (
                stack_event.logical_resource_id.as_deref().unwrap(),
                stack_event.resource_status.as_deref().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        resource_statuses,
        vec![
            (stack_name.as_str(), "UPDATE_IN_PROGRESS"),
            ("Vpc", "CREATE_FAILED"),
            (stack_name.as_str(), "UPDATE_ROLLBACK_IN_PROGRESS"),
            (
                stack_name.as_str(),
                "UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS"
            ),
            ("Vpc", "DELETE_COMPLETE"),
            (stack_name.as_str(), "UPDATE_ROLLBACK_COMPLETE")
        ]
    );

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: change_set.stack_id.unwrap(),
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
