#!/bin/sh
set -eu

tls_directory="$(mktemp -d /tmp/weavelit-local-tls.XXXXXX)"
server_pid=
relay_pid=

cleanup() {
  exit_status=$?
  trap - 0 HUP INT TERM

  if [ -n "$relay_pid" ]; then
    kill -TERM "$relay_pid" 2>/dev/null || true
    wait "$relay_pid" 2>/dev/null || true
  fi
  if [ -n "$server_pid" ]; then
    kill -TERM "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$tls_directory" 2>/dev/null || true

  exit "$exit_status"
}

trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

chmod 700 "$tls_directory"

certificate_path="$tls_directory/certificate.pem"
private_key_path="$tls_directory/private-key.pem"

openssl req -x509 -newkey rsa:2048 -sha256 -days 1 -noenc \
  -subj /CN=localhost \
  -addext subjectAltName=DNS:localhost,IP:127.0.0.1 \
  -keyout "$private_key_path" \
  -out "$certificate_path" \
  >/dev/null 2>&1
chmod 600 "$private_key_path"
chmod 644 "$certificate_path"

# The Server only accepts a loopback listener. Relay the Docker-published
# container port to that listener so the host remains the only external client.
socat TCP-LISTEN:8444,bind=0.0.0.0,reuseaddr,fork TCP:127.0.0.1:8443 &
relay_pid=$!

export WEAVELIT_HTTPS_LISTENER_ADDRESS=127.0.0.1:8443
export WEAVELIT_TLS_CERTIFICATE_PATH="$certificate_path"
export WEAVELIT_TLS_PRIVATE_KEY_PATH="$private_key_path"

target/release/weavelit-server &
server_pid=$!

while ! socat -u /dev/null TCP:127.0.0.1:8443,connect-timeout=1; do
  if ! kill -0 "$server_pid" 2>/dev/null; then
    if wait "$server_pid"; then
      server_status=0
    else
      server_status=$?
    fi
    server_pid=
    exit "$server_status"
  fi
done

touch /tmp/weavelit-local-server-ready
if wait "$server_pid"; then
  server_status=0
else
  server_status=$?
fi
server_pid=
exit "$server_status"
