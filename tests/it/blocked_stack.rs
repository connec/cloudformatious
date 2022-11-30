use std::convert::TryFrom;

use cloudformatious::BlockedStackStatus;

use crate::common::{clean_up, get_client, stack_with_status};

#[tokio::test]
async fn create_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::create_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::CreateFailed);

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn rollback_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::rollback_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::RollbackFailed);

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn delete_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::delete_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::DeleteFailed);

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn update_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::update_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::UpdateFailed);

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn update_rollback_failed() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;
    let failure = stack_with_status::update_rollback_failed(&client).await;

    let status = BlockedStackStatus::try_from(failure.stack_status).unwrap();
    assert_eq!(status, BlockedStackStatus::UpdateRollbackFailed);

    clean_up(failure.stack_id).await?;

    Ok(())
}
