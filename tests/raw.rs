use std::{env, error::Error, time::Duration};

use rusoto_cloudformation::{
    CloudFormation, CloudFormationClient, CreateChangeSetInput, CreateStackInput, DeleteStackInput,
    ExecuteChangeSetInput, StackEvent, Tag, UpdateStackInput,
};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};
use tokio_stream::StreamExt;

use rusoto_cloudformation_ext::raw::{
    CloudFormationExt, CreateChangeSetCheckedError, CreateStackCheckedError,
    CreateStackStreamError, ExecuteChangeSetStreamError, UpdateStackCheckedError,
    UpdateStackStreamError,
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
async fn create_stack_checked() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Successful create
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack = client.create_stack_checked(create_stack_input).await?;
    assert_eq!(stack.stack_status.as_str(), "CREATE_COMPLETE");

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: stack.stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    // Failed create
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let error = client
        .create_stack_checked(create_stack_input.clone())
        .await
        .unwrap_err();
    let stack = if let CreateStackCheckedError::Failed { status, stack } = error {
        assert_eq!(status, "ROLLBACK_COMPLETE");
        stack
    } else {
        return Err(error.into());
    };

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: stack.stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_stream() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Successful create
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack_events: Vec<StackEvent> = client
        .create_stack_stream(create_stack_input)
        .collect::<Result<_, _>>()
        .await?;
    let resource_statuses: Vec<_> = stack_events
        .iter()
        .map(|stack_event| stack_event.resource_status.as_deref().unwrap())
        .collect();
    assert_eq!(
        resource_statuses,
        vec!["CREATE_IN_PROGRESS", "CREATE_COMPLETE"]
    );

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: stack_events.last().unwrap().stack_name.to_string(),
            ..DeleteStackInput::default()
        })
        .await?;

    // Failed create
    let stack_name = generated_name();
    let create_stack_input = CreateStackInput {
        stack_name: stack_name.clone(),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack_events: Vec<Result<_, _>> = client
        .create_stack_stream(create_stack_input)
        .collect()
        .await;
    let resource_statuses = stack_events
        .iter()
        .map(|result| match result {
            Ok(stack_event) | Err(CreateStackStreamError::Failed { stack_event, .. }) => Ok((
                stack_event.logical_resource_id.as_deref().unwrap(),
                stack_event.resource_status.as_deref().unwrap(),
            )),
            Err(error) => Err(error),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        resource_statuses,
        vec![
            (stack_name.as_str(), "CREATE_IN_PROGRESS"),
            ("Vpc", "CREATE_FAILED"),
            (stack_name.as_str(), "ROLLBACK_IN_PROGRESS"),
            ("Vpc", "DELETE_COMPLETE"),
            (stack_name.as_str(), "ROLLBACK_COMPLETE")
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
async fn update_stack_checked() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Create a stack to update.
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack_name = client
        .create_stack_checked(create_stack_input)
        .await?
        .stack_name;

    // Successful update
    let update_stack_input = UpdateStackInput {
        stack_name,
        tags: Some(vec![Tag {
            key: "foo".to_string(),
            value: "bar".to_string(),
        }]),
        use_previous_template: Some(true),
        ..UpdateStackInput::default()
    };
    let stack = client.update_stack_checked(update_stack_input).await?;
    assert_eq!(stack.stack_status, "UPDATE_COMPLETE");

    // Failed update
    let update_stack_input = UpdateStackInput {
        stack_name: stack.stack_name.clone(),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        ..UpdateStackInput::default()
    };
    let error = client
        .update_stack_checked(update_stack_input.clone())
        .await
        .unwrap_err();
    let stack = if let UpdateStackCheckedError::Failed { status, stack } = error {
        assert_eq!(status, "UPDATE_ROLLBACK_COMPLETE");
        stack
    } else {
        return Err(error.into());
    };

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: stack.stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn update_stack_stream() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Create a stack to update.
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack_name = client
        .create_stack_checked(create_stack_input)
        .await?
        .stack_name;

    // Successful update
    let update_stack_input = UpdateStackInput {
        stack_name: stack_name.clone(),
        tags: Some(vec![Tag {
            key: "foo".to_string(),
            value: "bar".to_string(),
        }]),
        use_previous_template: Some(true),
        ..UpdateStackInput::default()
    };
    let stack_events: Vec<StackEvent> = client
        .update_stack_stream(update_stack_input)
        .collect::<Result<_, _>>()
        .await?;
    let resource_statuses: Vec<_> = stack_events
        .iter()
        .map(|stack_event| stack_event.resource_status.as_deref().unwrap())
        .collect();
    assert_eq!(
        resource_statuses,
        vec![
            "UPDATE_IN_PROGRESS",
            "UPDATE_COMPLETE_CLEANUP_IN_PROGRESS",
            "UPDATE_COMPLETE"
        ]
    );

    // Failed update
    let update_stack_input = UpdateStackInput {
        stack_name: stack_name.clone(),
        template_body: Some(FAILING_TEMPLATE.to_string()),
        ..UpdateStackInput::default()
    };
    let stack_events: Vec<Result<_, _>> = client
        .update_stack_stream(update_stack_input)
        .collect()
        .await;
    let resource_statuses = stack_events
        .iter()
        .map(|result| match result {
            Ok(stack_event) | Err(UpdateStackStreamError::Failed { stack_event, .. }) => Ok((
                stack_event.logical_resource_id.as_deref().unwrap(),
                stack_event.resource_status.as_deref().unwrap(),
            )),
            Err(error) => Err(error),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
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
            stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn delete_stack_checked() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Create a stack to delete
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack = client.create_stack_checked(create_stack_input).await?;

    // Successful delete
    let delete_stack_input = DeleteStackInput {
        stack_name: stack.stack_id.unwrap(),
        ..DeleteStackInput::default()
    };
    let stack = client
        .delete_stack_checked(delete_stack_input.clone())
        .await?;
    assert_eq!(stack.stack_status, "DELETE_COMPLETE");

    // Delete idempotent with id
    let stack = client
        .delete_stack_checked(delete_stack_input.clone())
        .await?;
    assert_eq!(stack.stack_status, "DELETE_COMPLETE");

    // Delete fails with name
    let delete_stack_input = DeleteStackInput {
        stack_name: stack.stack_name,
        ..DeleteStackInput::default()
    };
    let stack_result = client.delete_stack_checked(delete_stack_input).await;
    assert!(stack_result.is_err());

    Ok(())
}

#[tokio::test]
async fn delete_stack_stream() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    // Create a stack to delete
    let stack_name = generated_name();
    let create_stack_input = CreateStackInput {
        stack_name: stack_name.clone(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack = client.create_stack_checked(create_stack_input).await?;

    // Successful delete
    let delete_stack_input = DeleteStackInput {
        stack_name: stack.stack_id.unwrap(),
        ..DeleteStackInput::default()
    };
    let stack_events: Vec<StackEvent> = client
        .delete_stack_stream(delete_stack_input.clone())
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
            (stack_name.as_str(), "DELETE_IN_PROGRESS"),
            (stack_name.as_str(), "DELETE_COMPLETE")
        ]
    );

    // Delete idempotent with id
    let stack_events: Vec<StackEvent> = client
        .delete_stack_stream(delete_stack_input)
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
    assert_eq!(resource_statuses, Vec::<(&str, &str)>::new());

    // Delete fails with name
    let delete_stack_input = DeleteStackInput {
        stack_name: stack.stack_name,
        ..DeleteStackInput::default()
    };
    let results = client
        .delete_stack_stream(delete_stack_input)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(results.len(), 1);
    assert!(results.first().unwrap().is_err());

    Ok(())
}

#[tokio::test]
async fn create_change_set_checked() -> Result<(), Box<dyn Error>> {
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
        .create_change_set_checked(create_change_set_input)
        .await?;
    assert_eq!(change_set.execution_status.as_deref(), Some("AVAILABLE"));
    assert_eq!(change_set.status.as_deref(), Some("CREATE_COMPLETE"));

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: change_set.stack_id.unwrap(),
            ..DeleteStackInput::default()
        })
        .await?;

    // Failed create
    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack = client.create_stack_checked(create_stack_input).await?;

    let create_change_set_input = CreateChangeSetInput {
        stack_name: stack.stack_id.unwrap(),
        change_set_name: generated_name(),
        change_set_type: Some("UPDATE".to_string()),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateChangeSetInput::default()
    };
    let error = client
        .create_change_set_checked(create_change_set_input)
        .await
        .unwrap_err();
    if let CreateChangeSetCheckedError::Failed { status, .. } = error {
        assert_eq!(status, "FAILED");
    } else {
        return Err(error.into());
    }

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: stack.stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    // CreateChangeSet error
    let create_change_set_input = CreateChangeSetInput::default();
    let change_set_result = client
        .create_change_set_checked(create_change_set_input)
        .await;
    assert!(matches!(
        change_set_result,
        Err(CreateChangeSetCheckedError::CreateChangeSet(_))
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
        .create_change_set_checked(create_change_set_input)
        .await?;

    // Successful execution
    let execute_change_set_input = ExecuteChangeSetInput {
        change_set_name: change_set.change_set_id.unwrap(),
        ..ExecuteChangeSetInput::default()
    };
    let stack_events: Vec<_> = client
        .execute_change_set_stream(execute_change_set_input)
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
        .create_change_set_checked(create_change_set_input)
        .await?;

    // Failed execution
    let execute_change_set_input = ExecuteChangeSetInput {
        change_set_name: change_set.change_set_id.unwrap(),
        ..ExecuteChangeSetInput::default()
    };
    let stack_events: Vec<_> = client
        .execute_change_set_stream(execute_change_set_input)
        .collect()
        .await;
    let resource_statuses = stack_events
        .iter()
        .map(|result| match result {
            Ok(stack_event) | Err(ExecuteChangeSetStreamError::Failed { stack_event, .. }) => Ok((
                stack_event.logical_resource_id.as_deref().unwrap(),
                stack_event.resource_status.as_deref().unwrap(),
            )),
            Err(error) => Err(error),
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
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
