use assert_matches::assert_matches;
use cloudformatious::{
    ApplyStackError, ApplyStackInput, DeleteStackError, DeleteStackInput, Parameter, StackFailure,
    StackStatus, TemplateSource,
};

use crate::common::{get_role_arn, TestingRole, EMPTY_TEMPLATE};

use super::{generated_name, get_client, NON_EMPTY_TEMPLATE};

const FAILING_TEMPLATE: &str = r#"{
    "Resources": {
        "Bucket": {
            "Type": "AWS::S3::Bucket",
            "Properties": {}
        }
    }

}"#;

pub const ROLLBACK_FAILING_TEMPLATE: &str = r#"{
    "Parameters": {
        "CidrBlock": {
            "Type": "String"
        }
    },
    "Resources": {
        "Subnet": {
            "Type": "AWS::EC2::Subnet",
            "Properties": {
                "CidrBlock": {"Ref": "CidrBlock"},
                "Tags": [
                    {
                        "Key": "Foo",
                        "Value": "Bar"
                    }
                ],
                "VpcId": {"Fn::ImportValue": "cloudformatious-testing-VpcId"}
            }
        },
        "Bucket": {
            "Type": "AWS::S3::Bucket",
            "DependsOn": ["Subnet"],
            "Properties": {}
        }
    }
}"#;

pub async fn create_failed() -> StackFailure {
    let client = get_client().await;
    let error = client
        .apply_stack(
            ApplyStackInput::new(generated_name(), TemplateSource::inline(FAILING_TEMPLATE))
                .set_disable_rollback(true),
        )
        .await
        .unwrap_err();
    let failure = assert_matches!(error, ApplyStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::CreateFailed);
    failure
}

pub async fn rollback_failed() -> StackFailure {
    let client = get_client().await;
    let error = client
        .apply_stack(
            ApplyStackInput::new(
                generated_name(),
                TemplateSource::inline(ROLLBACK_FAILING_TEMPLATE),
            )
            .set_parameters([Parameter {
                key: "CidrBlock".to_string(),
                value: "10.0.0.32/28".to_string(),
            }])
            .set_role_arn(get_role_arn(TestingRole::DenyDeleteSubnet).await),
        )
        .await
        .unwrap_err();
    let failure = assert_matches!(error, ApplyStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::RollbackFailed);
    failure
}

pub async fn delete_failed() -> StackFailure {
    let client = get_client().await;
    let output = client
        .apply_stack(
            ApplyStackInput::new(generated_name(), TemplateSource::inline(NON_EMPTY_TEMPLATE))
                .set_parameters([Parameter {
                    key: "CidrBlock".to_string(),
                    value: "10.0.0.48/28".to_string(),
                }]),
        )
        .await
        .unwrap();

    let error = client
        .delete_stack(
            DeleteStackInput::new(&output.stack_id)
                .set_role_arn(get_role_arn(TestingRole::DenyDeleteSubnet).await),
        )
        .await
        .unwrap_err();
    let failure = assert_matches!(error, DeleteStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::DeleteFailed);
    failure
}

pub async fn update_failed() -> StackFailure {
    let client = get_client().await;
    let output = client
        .apply_stack(ApplyStackInput::new(
            generated_name(),
            TemplateSource::inline(EMPTY_TEMPLATE),
        ))
        .await
        .unwrap();

    let error = client
        .apply_stack(
            ApplyStackInput::new(output.stack_id, TemplateSource::inline(FAILING_TEMPLATE))
                .set_disable_rollback(true),
        )
        .await
        .unwrap_err();

    let failure = assert_matches!(error, ApplyStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::UpdateFailed);
    failure
}

pub async fn update_rollback_failed() -> StackFailure {
    let client = get_client().await;
    let output = client
        .apply_stack(
            ApplyStackInput::new(generated_name(), TemplateSource::inline(NON_EMPTY_TEMPLATE))
                .set_parameters([Parameter {
                    key: "CidrBlock".to_string(),
                    value: "10.0.0.80/28".to_string(),
                }]),
        )
        .await
        .unwrap();

    let error = client
        .apply_stack(
            ApplyStackInput::new(
                output.stack_id,
                TemplateSource::inline(ROLLBACK_FAILING_TEMPLATE),
            )
            .set_parameters([Parameter {
                key: "CidrBlock".to_string(),
                value: "10.0.0.80/28".to_string(),
            }]),
        )
        .await
        .unwrap_err();

    let failure = assert_matches!(error, ApplyStackError::Failure(failure) => failure);
    assert_eq!(failure.stack_status, StackStatus::UpdateRollbackFailed);
    failure
}
