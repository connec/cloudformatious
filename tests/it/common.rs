use std::{env, time::Duration};

use rusoto_cloudformation::CloudFormationClient;
use rusoto_core::{HttpClient, Region};
use rusoto_credential::{AutoRefreshingProvider, ChainProvider};

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

pub fn get_client() -> CloudFormationClient {
    get_arbitrary_client(CloudFormationClient::new_with)
}

pub fn get_arbitrary_client<F, T>(f: F) -> T
where
    F: FnOnce(HttpClient, AutoRefreshingProvider<ChainProvider>, Region) -> T,
{
    let client = HttpClient::new().unwrap();

    let mut credentials = AutoRefreshingProvider::new(ChainProvider::new()).unwrap();
    credentials.get_mut().set_timeout(Duration::from_secs(1));

    let region = env::var("AWS_REGION").expect("You must set AWS_REGION to run these tests");
    let region = region.parse().expect("Invalid AWS region");

    f(client, credentials, region)
}

pub fn generated_name() -> String {
    format!("{}{}", NAME_PREFIX, fastrand::u32(..))
}

pub async fn clean_up(
    client: &CloudFormationClient,
    stack_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    use rusoto_cloudformation::{CloudFormation, DeleteStackInput};
    CloudFormation::delete_stack(
        client,
        DeleteStackInput {
            stack_name,
            ..DeleteStackInput::default()
        },
    )
    .await
    .map_err(Into::into)
}
