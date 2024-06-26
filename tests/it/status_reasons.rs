use assert_matches::assert_matches;

use cloudformatious::{
    status_reason::StatusReasonDetail, ApplyStackError, ApplyStackInput, ResourceStatus,
    StackStatus, TemplateSource,
};

use crate::common::{
    clean_up, generated_name, get_client, get_sdk_config, MISSING_PERMISSION_TEMPLATE,
};

#[tokio::test]
async fn status_reason_missing_permission_with_principal() -> Result<(), Box<dyn std::error::Error>>
{
    let config = get_sdk_config().await;
    let identity_client = aws_sdk_sts::Client::new(&config);
    let identity = identity_client.get_caller_identity().send().await.unwrap();

    let client = get_client().await;

    let stack_name = generated_name();
    let input = ApplyStackInput::new(
        &stack_name,
        TemplateSource::inline(MISSING_PERMISSION_TEMPLATE),
    );
    let error = client.apply_stack(input).await.unwrap_err();

    let failure = assert_matches!(error, ApplyStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::RollbackComplete);

    let status_reason = assert_matches!(
        &failure.resource_events[..],
        [(ResourceStatus::CreateFailed, status)] if status.logical_resource_id() == "Fs" => {
            status.resource_status_reason()
        }
    );
    let missing_permission = assert_matches!(
      status_reason.detail(),
      Some(StatusReasonDetail::MissingPermission(missing_permission)) => missing_permission
    );

    assert_eq!(
        missing_permission.permission,
        "elasticfilesystem:CreateFileSystem"
    );
    assert_eq!(missing_permission.principal, identity.arn.as_deref());
    assert!(missing_permission.encoded_authorization_message.is_none());

    clean_up(stack_name).await?;

    Ok(())
}
