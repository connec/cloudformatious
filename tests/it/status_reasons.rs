use assert_matches::assert_matches;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use cloudformatious::{
    status_reason::StatusReasonDetail, ApplyStackError, ApplyStackInput, CloudFormatious,
    ResourceStatus, StackStatus, TemplateSource,
};

use crate::common::{
    clean_up, generated_name, get_arbitrary_client, get_client, MISSING_PERMISSION_1_TEMPLATE,
    MISSING_PERMISSION_2_TEMPLATE,
};

#[tokio::test]
async fn status_reason_missing_permission_no_principal() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(
        &stack_name,
        TemplateSource::inline(MISSING_PERMISSION_1_TEMPLATE),
    );
    let error = client.apply_stack(input).await.unwrap_err();

    let failure = assert_matches!(error, ApplyStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::RollbackComplete);

    let status_reason = assert_matches!(
        &failure.resource_events[..],
        [(ResourceStatus::CreateFailed, status)] if status.logical_resource_id() == "Bucket" => {
            status.resource_status_reason()
        }
    );
    let missing_permission = assert_matches!(
      status_reason.detail(),
      Some(StatusReasonDetail::MissingPermission(missing_permission)) => missing_permission
    );

    assert_eq!(missing_permission.permission, "s3:CreateBucket");
    assert_eq!(missing_permission.principal, None);

    clean_up(&client, stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn status_reason_missing_permission_with_principal() -> Result<(), Box<dyn std::error::Error>>
{
    let identity_client = get_arbitrary_client(StsClient::new_with);
    let identity = identity_client
        .get_caller_identity(GetCallerIdentityRequest {})
        .await
        .unwrap();

    let client = get_client();

    let stack_name = generated_name();
    let input = ApplyStackInput::new(
        &stack_name,
        TemplateSource::inline(MISSING_PERMISSION_2_TEMPLATE),
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

    clean_up(&client, stack_name).await?;

    Ok(())
}
