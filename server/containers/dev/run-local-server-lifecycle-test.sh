#!/bin/sh
set -eu

launcher_path="$(CDPATH= cd "$(dirname "$0")" && pwd)/run-local-server.sh"
test_directory="$(mktemp -d "${TMPDIR:-/tmp}/weavelit-launcher-test.XXXXXX")"
fake_bin="$test_directory/fake-bin"
fake_server="$test_directory/fake-server"
active_launcher_pid=

cleanup_test() {
  exit_status=$?
  trap - 0 HUP INT TERM

  if [ -n "$active_launcher_pid" ]; then
    kill -TERM "$active_launcher_pid" 2>/dev/null || true
    wait "$active_launcher_pid" 2>/dev/null || true
  fi
  for pid_file in "$test_directory"/cases/*/state/*.pid; do
    if [ -f "$pid_file" ]; then
      process_pid="$(cat "$pid_file")"
      kill -TERM "$process_pid" 2>/dev/null || true
    fi
  done
  rm -rf "$test_directory"

  exit "$exit_status"
}

trap cleanup_test 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

fail() {
  printf 'run-local-server lifecycle test failed: %s\n' "$1" >&2
  exit 1
}

assert_status() {
  expected_status=$1
  actual_status=$2
  context=$3
  if [ "$actual_status" -ne "$expected_status" ]; then
    fail "$context returned $actual_status instead of $expected_status"
  fi
}

assert_exists() {
  asserted_path=$1
  context=$2
  if [ ! -e "$asserted_path" ]; then
    fail "$context did not create $asserted_path"
  fi
}

assert_absent() {
  asserted_path=$1
  context=$2
  if [ -e "$asserted_path" ]; then
    fail "$context left $asserted_path behind"
  fi
}

assert_process_stopped() {
  pid_file=$1
  context=$2
  assert_exists "$pid_file" "$context"
  process_pid="$(cat "$pid_file")"
  if kill -0 "$process_pid" 2>/dev/null; then
    fail "$context left process $process_pid running"
  fi
  rm -f "$pid_file"
}

wait_for_file() {
  awaited_path=$1
  context=$2
  attempt=0
  while [ ! -e "$awaited_path" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -eq 100 ]; then
      fail "$context did not create $awaited_path"
    fi
    sleep 0.05
  done
}

mkdir -p "$fake_bin" "$test_directory/cases"

cat >"$fake_bin/mktemp" <<'EOF'
#!/bin/sh
set -eu
mkdir "$FAKE_TLS_DIRECTORY"
printf '%s\n' "$FAKE_TLS_DIRECTORY"
EOF

cat >"$fake_bin/openssl" <<'EOF'
#!/bin/sh
set -eu

certificate_path=
private_key_path=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -keyout)
      shift
      private_key_path=$1
      ;;
    -out)
      shift
      certificate_path=$1
      ;;
  esac
  shift
done

[ -n "$certificate_path" ]
[ -n "$private_key_path" ]
: >"$certificate_path"
: >"$private_key_path"
exit "${FAKE_OPENSSL_STATUS:-0}"
EOF

cat >"$fake_bin/socat" <<'EOF'
#!/bin/sh
set -eu

case "${1:-}" in
  TCP-LISTEN:*)
    relay_terminated() {
      : >"$FAKE_STATE/relay.terminated"
      exit 0
    }
    trap relay_terminated HUP INT TERM
    printf '%s\n' "$$" >"$FAKE_STATE/relay.pid"
    : >"$FAKE_STATE/relay.started"
    while :; do
      sleep 1
    done
    ;;
  *)
    exit "${FAKE_PROBE_STATUS:-0}"
    ;;
esac
EOF

cat >"$fake_bin/touch" <<'EOF'
#!/bin/sh
set -eu
: >"$FAKE_STATE/ready"
EOF

cat >"$fake_server" <<'EOF'
#!/bin/sh
set -eu

server_terminated() {
  : >"$FAKE_STATE/server.terminated"
  exit 0
}

trap server_terminated HUP INT TERM
printf '%s\n' "$$" >"$FAKE_STATE/server.pid"
: >"$FAKE_STATE/server.started"

if [ "$FAKE_SERVER_MODE" = exit ]; then
  while [ ! -e "$FAKE_STATE/relay.started" ]; do
    sleep 0.05
  done
  exit "$FAKE_SERVER_STATUS"
fi

while :; do
  sleep 1
done
EOF

chmod +x "$fake_bin/mktemp" "$fake_bin/openssl" "$fake_bin/socat" \
  "$fake_bin/touch" "$fake_server"

prepare_case() {
  case_name=$1
  case_directory="$test_directory/cases/$case_name"
  mkdir -p "$case_directory/state" "$case_directory/target/release"
  ln -s "$fake_server" "$case_directory/target/release/weavelit-server"
}

run_foreground_case() {
  foreground_case_directory=$1
  openssl_status=$2
  server_status=$3
  probe_status=$4

  if (
    cd "$foreground_case_directory"
    exec env \
      PATH="$fake_bin:$PATH" \
      FAKE_STATE="$foreground_case_directory/state" \
      FAKE_TLS_DIRECTORY="$foreground_case_directory/tls" \
      FAKE_OPENSSL_STATUS="$openssl_status" \
      FAKE_SERVER_MODE=exit \
      FAKE_SERVER_STATUS="$server_status" \
      FAKE_PROBE_STATUS="$probe_status" \
      sh "$launcher_path"
  ); then
    launcher_status=0
  else
    launcher_status=$?
  fi
}

prepare_case openssl-failure
openssl_failure_directory=$case_directory
run_foreground_case "$openssl_failure_directory" 23 0 0
assert_status 23 "$launcher_status" "OpenSSL failure"
assert_absent "$openssl_failure_directory/tls" "OpenSSL failure"
assert_absent "$openssl_failure_directory/state/relay.started" "OpenSSL failure"
assert_absent "$openssl_failure_directory/state/server.started" "OpenSSL failure"

prepare_case server-success
server_success_directory=$case_directory
run_foreground_case "$server_success_directory" 0 0 0
assert_status 0 "$launcher_status" "successful Server exit"
assert_absent "$server_success_directory/tls" "successful Server exit"
assert_exists "$server_success_directory/state/relay.terminated" "successful Server exit"
assert_process_stopped "$server_success_directory/state/relay.pid" "successful Server exit"
assert_process_stopped "$server_success_directory/state/server.pid" "successful Server exit"

prepare_case server-failure
server_failure_directory=$case_directory
run_foreground_case "$server_failure_directory" 0 42 1
assert_status 42 "$launcher_status" "failed Server exit"
assert_absent "$server_failure_directory/tls" "failed Server exit"
assert_exists "$server_failure_directory/state/relay.terminated" "failed Server exit"
assert_process_stopped "$server_failure_directory/state/relay.pid" "failed Server exit"
assert_process_stopped "$server_failure_directory/state/server.pid" "failed Server exit"

prepare_case termination
termination_directory=$case_directory
(
  cd "$termination_directory"
  exec env \
    PATH="$fake_bin:$PATH" \
    FAKE_STATE="$termination_directory/state" \
    FAKE_TLS_DIRECTORY="$termination_directory/tls" \
    FAKE_OPENSSL_STATUS=0 \
    FAKE_SERVER_MODE=wait \
    FAKE_SERVER_STATUS=0 \
    sh "$launcher_path"
) &
active_launcher_pid=$!

wait_for_file "$termination_directory/state/ready" "termination case"
kill -TERM "$active_launcher_pid"
if wait "$active_launcher_pid"; then
  launcher_status=0
else
  launcher_status=$?
fi
active_launcher_pid=

assert_status 143 "$launcher_status" "terminated launcher"
assert_absent "$termination_directory/tls" "terminated launcher"
assert_exists "$termination_directory/state/relay.terminated" "terminated launcher"
assert_exists "$termination_directory/state/server.terminated" "terminated launcher"
assert_process_stopped "$termination_directory/state/relay.pid" "terminated launcher"
assert_process_stopped "$termination_directory/state/server.pid" "terminated launcher"

printf 'run-local-server lifecycle tests passed\n'