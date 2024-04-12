#!/usr/bin/env bash

set -euo pipefail

stacks=$(
  aws cloudformation list-stacks \
    --stack-status-filter REVIEW_IN_PROGRESS ROLLBACK_FAILED ROLLBACK_COMPLETE UPDATE_ROLLBACK_COMPLETE \
    --query "StackSummaries[?starts_with(StackName, 'cloudformatious-testing-')].StackName" \
    --output text
)
for stack in $stacks ; do
  aws cloudformation delete-stack --stack-name "$stack"
done
