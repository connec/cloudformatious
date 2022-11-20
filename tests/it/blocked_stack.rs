use crate::common::{clean_up, stack_with_status};

#[tokio::test]
async fn create_failed() -> Result<(), Box<dyn std::error::Error>> {
    let failure = stack_with_status::create_failed().await;

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn rollback_failed() -> Result<(), Box<dyn std::error::Error>> {
    let failure = stack_with_status::rollback_failed().await;

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn delete_failed() -> Result<(), Box<dyn std::error::Error>> {
    let failure = stack_with_status::delete_failed().await;

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn update_failed() -> Result<(), Box<dyn std::error::Error>> {
    let failure = stack_with_status::update_failed().await;

    clean_up(failure.stack_id).await?;

    Ok(())
}

#[tokio::test]
async fn update_rollback_failed() -> Result<(), Box<dyn std::error::Error>> {
    let failure = stack_with_status::update_rollback_failed().await;

    clean_up(failure.stack_id).await?;

    Ok(())
}
