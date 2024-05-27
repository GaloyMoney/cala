#!/usr/bin/env bats

load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
}

@test "atomic-operation: check balance updates for an account set" {
  id=$(random_uuid)
  another_id=$(random_uuid)

  variables=$(
    jq -n \
    --arg id "$id" \
    --arg anotherId "$another_id" \
    '{
       "id": $id,
       "anotherId": $anotherId
    }'
  )
  exec_graphql 'multi-journal-create' "$variables"
  echo $(graphql_output)
  first=$(graphql_output '.data.first')
  second=$(graphql_output '.data.second')
  [[ "$first" != "null" ]] || exit 1
  [[ "$second" != "null" ]] || exit 1

  id=$(random_uuid)
  variables=$(
    jq -n \
    --arg id "$id" \
    --arg anotherId "$another_id" \
    '{
       "id": $id,
       "anotherId": $anotherId
    }'
  )
  exec_graphql 'multi-journal-create' "$variables"
  data=$(graphql_output '.data')
  [[ "$data" = "null" ]] || exit 1
}
