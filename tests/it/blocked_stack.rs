use std::convert::TryFrom;

use assert_matches::assert_matches;
use cloudformatious::{ApplyStackError, ApplyStackInput, BlockedStackStatus, Client, Parameter};

use crate::common::{clean_up, get_client, stack_with_status, NON_EMPTY_TEMPLATE};

#[tokio::test]
async fn create_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::create_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::CreateFailed);

    let error = try_update(&client, &failure.stack_id).await;
    assert_matches!(
        error,
        ApplyStackError::Blocked {
            status: BlockedStackStatus::CreateFailed,
        }
    );

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn rollback_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::rollback_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::RollbackFailed);

    let error = try_update(&client, &failure.stack_id).await;
    assert_matches!(
        error,
        ApplyStackError::Blocked {
            status: BlockedStackStatus::RollbackFailed,
        }
    );

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn delete_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::delete_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::DeleteFailed);

    let error = try_update(&client, &failure.stack_id).await;
    assert_matches!(
        error,
        ApplyStackError::Blocked {
            status: BlockedStackStatus::DeleteFailed,
        }
    );

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn update_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::update_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::UpdateFailed);

    let error = try_update(&client, &failure.stack_id).await;
    assert_matches!(
        error,
        ApplyStackError::Blocked {
            status: BlockedStackStatus::UpdateFailed,
        }
    );

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn update_rollback_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::update_rollback_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::UpdateRollbackFailed);

    let error = try_update(&client, &failure.stack_id).await;
    assert_matches!(
        error,
        ApplyStackError::Blocked {
            status: BlockedStackStatus::UpdateRollbackFailed,
        }
    );

    clean_up(failure.stack_id).await?;

    Ok(())
}

async fn try_update(client: &Client, stack_name: &str) -> ApplyStackError {
    client
        .apply_stack(
            ApplyStackInput::new(
                stack_name,
                cloudformatious::TemplateSource::inline(NON_EMPTY_TEMPLATE),
            )
            .set_parameters([Parameter {
                key: "CidrBlock".to_string(),
                value: "10.0.0.0/28".to_string(),
            }]),
        )
        .await
        .unwrap_err()
}
