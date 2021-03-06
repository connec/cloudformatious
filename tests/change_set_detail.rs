pub mod common;

use enumset::EnumSet;
use rusoto_cloudformation::Tag;

use cloudformatious::{
    change_set::{
        Action, Evaluation, ModifyDetail, ModifyScope, Replacement, ResourceChange,
        ResourceChangeDetail, ResourceTargetDefinition,
    },
    ApplyStackInput, CloudFormatious, Parameter, TemplateSource,
};

use common::{clean_up, generated_name, get_client, NON_EMPTY_TEMPLATE};

#[tokio::test]
async fn changes_tags_only() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client();

    let stack_name = generated_name();
    let mut input = ApplyStackInput::new(&stack_name, TemplateSource::inline(NON_EMPTY_TEMPLATE))
        .set_parameters([Parameter {
            key: "CidrBlock".to_string(),
            value: "10.0.0.16/28".to_string(),
        }]);
    let output = client.apply_stack(input.clone()).await?;
    let subnet_id = output
        .outputs
        .into_iter()
        .find(|output| output.key == "SubnetId")
        .expect("missing SubnetId output")
        .value;

    input = input.set_tags([Tag {
        key: "hello".to_string(),
        value: "world".to_string(),
    }]);
    let change_set = client.apply_stack(input).change_set().await?;

    assert_eq!(
        change_set.changes,
        vec![ResourceChange {
            action: Action::Modify(ModifyDetail {
                details: vec![ResourceChangeDetail {
                    change_source: None,
                    evaluation: Evaluation::Static,
                    target: ResourceTargetDefinition::Tags,
                }],
                replacement: Replacement::False,
                scope: EnumSet::only(ModifyScope::Tags),
            },),
            logical_resource_id: "Subnet".to_string(),
            physical_resource_id: Some(subnet_id),
            resource_type: "AWS::EC2::Subnet".to_string(),
        }]
    );

    clean_up(&client, stack_name).await?;

    Ok(())
}
