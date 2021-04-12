use std::{env, error::Error, time::Duration};

use rusoto_cloudformation::{
    CloudFormation, CloudFormationClient, CreateChangeSetInput, CreateStackInput, DeleteStackInput,
    Tag, UpdateStackInput,
};
use rusoto_core::HttpClient;
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};

use rusoto_cloudformation_ext::raw::{
    CloudFormationExt, CreateChangeSetCheckedError, CreateStackCheckedError,
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

#[tokio::test]
async fn create_stack_checked() -> Result<(), Box<dyn Error>> {
    let client = get_client();

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

    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(
            r#"
            {
                "Resources": {
                    "Vpc": {
                        "Type": "AWS::EC2::VPC",
                        "Properties": {}
                    }
                }
            }
            "#
            .to_string(),
        ),
        ..CreateStackInput::default()
    };
    let stack_result = client
        .create_stack_checked(create_stack_input.clone())
        .await;
    assert!(stack_result.is_err());
    assert!(matches!(
        stack_result,
        Err(CreateStackCheckedError::Failed { .. })
    ));
    if let Err(CreateStackCheckedError::Failed { status, .. }) = stack_result {
        assert_eq!(status, "ROLLBACK_COMPLETE");
    }

    // Clean-up
    client
        .delete_stack(DeleteStackInput {
            stack_name: create_stack_input.stack_name,
            ..DeleteStackInput::default()
        })
        .await?;

    Ok(())
}

#[tokio::test]
async fn update_stack_checked() -> Result<(), Box<dyn Error>> {
    let client = get_client();

    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack_name = client
        .create_stack_checked(create_stack_input)
        .await?
        .stack_name;

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

    let create_stack_input = CreateStackInput {
        stack_name: generated_name(),
        template_body: Some(DUMMY_TEMPLATE.to_string()),
        ..CreateStackInput::default()
    };
    let stack_name = client
        .create_stack_checked(create_stack_input)
        .await?
        .stack_name;

    let delete_stack_input = DeleteStackInput {
        stack_name,
        ..DeleteStackInput::default()
    };
    let stack = client
        .delete_stack_checked(delete_stack_input.clone())
        .await?;
    assert_eq!(stack.stack_status, "DELETE_COMPLETE");

    let stack_result = client
        .delete_stack_checked(delete_stack_input.clone())
        .await;
    assert!(stack_result.is_err());

    Ok(())
}

#[tokio::test]
async fn create_change_set_checked() -> Result<(), Box<dyn Error>> {
    let client = get_client();

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

    let create_change_set_input = CreateChangeSetInput::default();
    let change_set_result = client
        .create_change_set_checked(create_change_set_input)
        .await;
    assert!(matches!(
        change_set_result,
        Err(CreateChangeSetCheckedError::CreateChangeSet(_))
    ));

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
