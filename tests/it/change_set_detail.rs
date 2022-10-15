use enumset::EnumSet;

use cloudformatious::{
    change_set::{
        Action, Evaluation, ModifyDetail, ModifyScope, Replacement, ResourceChange,
        ResourceChangeDetail, ResourceTargetDefinition,
    },
    ApplyStackInput, Parameter, Tag, TemplateSource,
};

use crate::common::{
    clean_up, generated_name, get_client, NON_EMPTY_TEMPLATE, SECRETS_MANAGER_SECRET,
};

#[tokio::test]
async fn changes_tags_only() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

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

    clean_up(stack_name).await?;

    Ok(())
}

#[tokio::test]
async fn secrets_manager_secret_tags_only() -> Result<(), Box<dyn std::error::Error>> {
    let client = get_client().await;

    let stack_name = generated_name();
    let mut input =
        ApplyStackInput::new(&stack_name, TemplateSource::inline(SECRETS_MANAGER_SECRET))
            .set_parameters([Parameter {
                key: "TagValue".to_string(),
                value: "a".to_string(),
            }]);

    client.apply_stack(input.clone()).await?;

    input.parameters[0].value = "b".to_string();
    let change_set = client.apply_stack(input).change_set().await?;

    let targets: Vec<_> = change_set
        .changes
        .into_iter()
        .filter_map(|change| match change.action {
            Action::Modify(details) => {
                Some(details.details.into_iter().map(|detail| detail.target))
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!targets.is_empty());
    assert!(targets
        .iter()
        .all(|target| matches!(target, ResourceTargetDefinition::Tags)));

    clean_up(stack_name).await?;

    Ok(())
}
