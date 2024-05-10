REPO_ROOT=$(git rev-parse --show-toplevel)
GQL_DIR="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/gql"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-${REPO_ROOT##*/}}"

GQL_ENDPOINT="http://localhost:2252/graphql"

CALA_HOME="${CALA_HOME:-.cala}"

reset_pg() {
  docker exec "${COMPOSE_PROJECT_NAME}-server-pg-1" psql $PG_CON -c "DROP SCHEMA public CASCADE"
  docker exec "${COMPOSE_PROJECT_NAME}-server-pg-1" psql $PG_CON -c "CREATE SCHEMA public"
  docker exec "${COMPOSE_PROJECT_NAME}-examples-pg-1" psql $PG_CON -c "DROP SCHEMA public CASCADE"
  docker exec "${COMPOSE_PROJECT_NAME}-examples-pg-1" psql $PG_CON -c "CREATE SCHEMA public"
}

server_cmd() {
  server_location="${REPO_ROOT}/target/debug/cala-server --config ${REPO_ROOT}/bats/cala.yml"
  if [[ ! -z ${CARGO_TARGET_DIR} ]] ; then
    server_location="${CARGO_TARGET_DIR}/debug/cala-server --config ${REPO_ROOT}/bats/cala.yml"
  fi

  ${server_location} $@
}

start_server() {
  background server_cmd > .e2e-logs
  for i in {1..20}
  do
    if head .e2e-logs | grep -q 'Starting graphql server on port'; then
      break
    else
      sleep 1
    fi
  done
}

stop_server() {
  if [[ -f ${CALA_HOME}/server-pid ]]; then
    kill -9 $(cat ${CALA_HOME}/server-pid) || true
  fi
}

stop_rust_example() {
  if [[ -f ${CALA_HOME}/rust-example-pid ]]; then
    kill -9 $(cat ${CALA_HOME}/rust-example-pid) || true
  fi
}

gql_file() {
  echo "${GQL_DIR}/$1.gql"
}

gql_query() {
  cat "$(gql_file $1)" | tr '\n' ' ' | sed 's/"/\\"/g'
}

graphql_output() {
  echo $output | jq -r "$@"
}

exec_graphql() {
  local query_name=$1
  local variables=${2:-"{}"}

  if [[ "${BATS_TEST_DIRNAME}" != "" ]]; then
    run_cmd="run"
  else
    run_cmd=""
  fi

  ${run_cmd} curl -s \
    -X POST \
    ${AUTH_HEADER:+ -H "$AUTH_HEADER"} \
    -H "Content-Type: application/json" \
    -d "{\"query\": \"$(gql_query $query_name)\", \"variables\": $variables}" \
    "${GQL_ENDPOINT}"
}

# Run the given command in the background. Useful for starting a
# node and then moving on with commands that exercise it for the
# test.
#
# Ensures that BATS' handling of file handles is taken into account;
# see
# https://github.com/bats-core/bats-core#printing-to-the-terminal
# https://github.com/sstephenson/bats/issues/80#issuecomment-174101686
# for details.
background() {
  "$@" 3>- &
  echo $!
}

# Taken from https://github.com/docker/swarm/blob/master/test/integration/helpers.bash
# Retry a command $1 times until it succeeds. Wait $2 seconds between retries.
retry() {
  local attempts=$1
  shift
  local delay=$1
  shift
  local i

  for ((i=0; i < attempts; i++)); do
    run "$@"
    if [[ "$status" -eq 0 ]] ; then
      return 0
    fi
    sleep "$delay"
  done

  echo "Command \"$*\" failed $attempts times. Output: $output"
  false
}

