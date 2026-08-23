#!/bin/sh
set -eu

tls_directory="$(mktemp -d /tmp/weavelit-local-tls.XXXXXX)"
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

export WEAVELIT_HTTPS_LISTENER_ADDRESS=127.0.0.1:8443
export WEAVELIT_TLS_CERTIFICATE_PATH="$certificate_path"
export WEAVELIT_TLS_PRIVATE_KEY_PATH="$private_key_path"

target/release/weavelit-server &
server_pid=$!

while ! socat -u /dev/null TCP:127.0.0.1:8443,connect-timeout=1; do
  if ! kill -0 "$server_pid" 2>/dev/null; then
    wait "$server_pid"
    exit $?
  fi
done

touch /tmp/weavelit-local-server-ready
wait "$server_pid"