use futures_util::StreamExt;

use cloudformatious::{
    change_set::{Action, ExecutionStatus},
    ApplyStackError, ApplyStackInput, ChangeSetStatus, CloudFormatious, ResourceStatus,
    StackFailure, StackStatus, TemplateSource,
};

use crate::common::{clean_up, generated_name, get_client, EMPTY_TEMPLATE};

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
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
    let output = client.apply_stack(input).await?;
    assert_eq!(output.stack_status, StackStatus::CreateComplete);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_change_set_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
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
                &change.action,
                change.logical_resource_id.as_str(),
                change.resource_type.as_str(),
            )
        })
        .collect();
    assert_eq!(changes, vec![(&Action::Add, "Vpc", "AWS::EC2::VPC")]);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_change_set_cancel_idempotent() -> Result<(), Box<dyn std::error::Error>> {
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
                &change.action,
                change.logical_resource_id.as_str(),
                change.resource_type.as_str(),
            )
        })
        .collect();
    assert_eq!(changes, vec![(&Action::Add, "Vpc", "AWS::EC2::VPC")]);

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
                &change.action,
                change.logical_resource_id.as_str(),
                change.resource_type.as_str(),
            )
        })
        .collect();
    assert_eq!(changes, vec![(&Action::Add, "Vpc", "AWS::EC2::VPC")]);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn create_stack_stream_ok() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
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
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
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
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));

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
        let resource_errors = resource_events
            .iter()
            .map(|(status, details)| {
                (
                    details.logical_resource_id(),
                    *status,
                    details.resource_status_reason().inner(),
                )
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            &resource_errors[..],
            [("Vpc", ResourceStatus::CreateFailed, Some(_))]
        ));
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
            ("Vpc".to_string(), "CREATE_IN_PROGRESS".to_string()),
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
        let resource_errors = resource_events
            .iter()
            .map(|(status, details)| {
                (
                    details.logical_resource_id(),
                    *status,
                    details.resource_status_reason().inner(),
                )
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            &resource_errors[..],
            [("Vpc", ResourceStatus::CreateFailed, Some(_))]
        ));
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
    let input = ApplyStackInput::new(&stack_name, TemplateSource::inline(EMPTY_TEMPLATE));
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
        let resource_errors = resource_events
            .iter()
            .map(|(status, details)| {
                (
                    details.logical_resource_id(),
                    *status,
                    details.resource_status_reason().inner(),
                )
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            &resource_errors[..],
            [("Vpc", ResourceStatus::CreateFailed, Some(_))]
        ));
    } else {
        return Err(error.into());
    }

    clean_up(&client, stack_name).await?;

    Ok(())
}
