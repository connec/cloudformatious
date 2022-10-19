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
                "Tags": [
                    {
                        "Key": "VpcId",
                        "Value": {"Fn::ImportValue": "cloudformatious-testing-VpcId"}
                    }
                ],
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

pub const MISSING_PERMISSION_1_TEMPLATE: &str = r#"{
    "Resources": {
        "Bucket": {
            "Type": "AWS::S3::Bucket",
            "Properties": {}
        }
    }
}"#;

pub const MISSING_PERMISSION_2_TEMPLATE: &str = r#"{
  "Resources": {
    "Fs": {
      "Type": "AWS::EFS::FileSystem",
      "Properties": {}
    }
  }
}"#;

pub const AUTHORIZATION_FAILURE_TEMPLATE: &str = r#"{
  "Resources": {
    "Vpc": {
      "Type": "AWS::EC2::VPC",
      "Properties": {
        "CidrBlock": "0.0.0.0/16"
      }
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
        .send()
        .await
        .map(|_| ())
        .map_err(Into::into)
}
