#!/usr/bin/env bats

load "helpers"

setup_file() {
  reset_pg_and_restart_server
}

teardown_file() {
  stop_server
  stop_nodejs_example
}

reset_pg_and_restart_server() {
  stop_server
  reset_pg
  PG_CON=$PG_CON_EXAMPLE start_server
}

@test "nodejs: entities sync to server" {
  reset_pg_and_restart_server

  exec_graphql 'list-accounts'
  accounts_before=$(graphql_output '.data.accounts.nodes | length')

  job_id=$(random_uuid)
  variables=$(
    jq -n \
      --arg jobId "$job_id" \
    '{
      input: {
        jobId: $jobId,
        endpoint: "http://localhost:2258"
      }
    }'
  )
  exec_graphql 'cala-outbox-import-job-create' "$variables"
  echo "GraphQL Response: $(graphql_output)"
  id=$(graphql_output '.data.calaOutboxImportJobCreate.job.jobId')
  error_msg=$(graphql_output '.errors[0].message')
  [[ "$id" == "$job_id" || "$error_msg" =~ duplicate.*jobs_name_key ]] || exit 1;

  background bash -c "cd ${REPO_ROOT}/examples/nodejs && npm run start > ${REPO_ROOT}/.nodejs-example-logs 2>&1" &
  NODEJS_EXAMPLE_PID=$!
  echo $NODEJS_EXAMPLE_PID > "${NODEJS_EXAMPLE_PID_FILE}"

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

  variables=$(
    jq -n \
      --arg code "RECORD_DEPOSIT" \
    '{"code": $code}'
  )
  exec_graphql 'tx-template-find-by-code' "$variables"
  tx_template_code=$(graphql_output '.data.txTemplateFindByCode.txTemplate.code')
  [[ "$tx_template_code" != "RECORD_DEPOSIT" ]] || exit 1
}
