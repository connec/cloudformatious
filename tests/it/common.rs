pub mod stack_with_status;

use aws_config::SdkConfig;
use cloudformatious::Client;

const NAME_PREFIX: &str = "cloudformatious-testing-";

pub const EMPTY_TEMPLATE: &str = r#"{
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

pub const EMPTY_TEMPLATE_WITH_TRANSFORM: &str = r#"{
    "Transform": "AWS::Serverless-2016-10-31",
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

pub const NON_EMPTY_TEMPLATE: &str = r#"{
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
                "VpcId": {"Fn::ImportValue": "cloudformatious-testing-VpcId"}
            }
        }
    },
    "Outputs": {
        "SubnetId": {
            "Value": {"Ref": "Subnet"}
        }
    }
}"#;

pub const MISSING_PERMISSION_TEMPLATE: &str = r#"{
  "Resources": {
    "Fs": {
      "Type": "AWS::EFS::FileSystem",
      "Properties": {}
    }
  }
}"#;

pub const SECRETS_MANAGER_SECRET: &str = r#"{
    "Parameters": {
        "TagValue": {
            "Type": "String"
        }
    },
    "Resources": {
        "Secret": {
            "Type": "AWS::SecretsManager::Secret",
            "Properties": {
                "Name": {
                    "Ref": "AWS::StackName"
                },
                "Tags": [
                    {
                        "Key": "Key",
                        "Value": {
                            "Ref": "TagValue"
                        }
                    }
                ]
            }
        }
    }
}"#;

pub async fn get_sdk_config() -> SdkConfig {
    aws_config::load_from_env().await
}

pub async fn get_client() -> Client {
    let config = aws_config::load_from_env().await;
    Client::new(&config)
}

pub fn generated_name() -> String {
    format!("{}{}", NAME_PREFIX, fastrand::u32(..))
}

pub async fn clean_up(stack_name: String) -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_cloudformation::Client::new(&config);
    client
        .delete_stack()
        .stack_name(stack_name)
        .role_arn(get_role_arn(TestingRole::Testing).await)
        .send()
        .await
        .map(|_| ())
        .map_err(Into::into)
}

pub enum TestingRole {
    Testing,
    DenyDeleteSubnet,
}

pub async fn get_role_arn(role: TestingRole) -> String {
    let sts = aws_sdk_sts::Client::new(&get_sdk_config().await);
    let account_id = sts
        .get_caller_identity()
        .send()
        .await
        .unwrap()
        .account
        .unwrap();
    format!(
        "arn:aws:iam::{account_id}:role/cloudformatious-testing{role_suffix}",
        role_suffix = match role {
            TestingRole::Testing => "",
            TestingRole::DenyDeleteSubnet => "-deny-delete-subnet",
        }
    )
}
