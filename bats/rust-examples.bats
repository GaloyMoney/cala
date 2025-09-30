#!/usr/bin/env bats

load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
  stop_rust_example
}

@test "rust: entities sync to server" {
  exec_graphql 'list-accounts'
  accounts_before=$(graphql_output '.data.accounts.nodes | length')

  job_id=$(random_uuid)
  variables=$(
    jq -n \
      --arg jobId "$job_id" \
    '{
      input: {
        jobId: $jobId,
        endpoint: "http://localhost:2253"
      }
    }'
  )
  exec_graphql 'cala-outbox-import-job-create' "$variables"
  id=$(graphql_output '.data.calaOutboxImportJobCreate.job.jobId')
  error_msg=$(graphql_output '.errors[0].message')
  echo $(graphql_output)
  [[ "$id" == "$job_id" || "$error_msg" =~ duplicate.*jobs_name_key ]] || exit 1;

  background cargo run --bin cala-ledger-example-rust > .rust-example-logs 2>&1

  job_count=$(cat .e2e-logs | grep 'Executing CalaOutboxImportJob importing' | wc -l)
  retry 30 1 wait_for_new_import_job $job_count || true
  sleep 1

  for i in {1..90}; do
    exec_graphql 'list-accounts'
    accounts_after=$(graphql_output '.data.accounts.nodes | length')
    if [[ "$accounts_after" -gt "$accounts_before" ]] then
      break;
    fi
    sleep 1
  done

  [[ "$accounts_after" -gt "$accounts_before" ]] || exit 1
}
