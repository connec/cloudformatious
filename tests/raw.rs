use std::{env, error::Error, time::Duration};

use rusoto_cloudformation::{
    CloudFormation, CloudFormationClient, CreateChangeSetInput, CreateStackInput, DeleteStackInput,
    Tag, UpdateStackInput,
};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};

use rusoto_cloudformation_ext::raw::{
    CloudFormationExt, CreateChangeSetCheckedError, CreateStackCheckedError,
    CreateStackStreamError, UpdateStackCheckedError,
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
