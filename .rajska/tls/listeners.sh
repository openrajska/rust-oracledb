#!/usr/bin/env bash
# Starts two TCPS listeners for the TLS tests, inside the database container
# (as the oracle user), next to the default listener and without touching it:
#   RAJSKA_TLS  on 2484: one-way TLS (the server's certificate only);
#   RAJSKA_MTLS on 2485: mutual TLS (the client must present a certificate).
# Both serve FREEPDB1 through static registration, with the server wallet in
# ./server. Each has a TNS_ADMIN of its own, which the server processes it
# starts inherit. Idempotent: listeners already running are restarted.
# Usage, from this folder copied into the container:
#   bash listeners.sh [service]        (default service: FREEPDB1)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
service="${1:-FREEPDB1}"
sid="${ORACLE_SID:-FREE}"
root=/tmp/rajska-tls

mkdir -p "$root/wallet"
cp "$here/server/cwallet.sso" "$here/server/ewallet.p12" "$root/wallet/"

start() {
    local name=$1 port=$2 client_auth=$3
    local admin="$root/$name"
    mkdir -p "$admin"
    cat > "$admin/sqlnet.ora" <<EOF
WALLET_LOCATION = (SOURCE = (METHOD = FILE)(METHOD_DATA = (DIRECTORY = $root/wallet)))
SSL_CLIENT_AUTHENTICATION = $client_auth
SQLNET.AUTHENTICATION_SERVICES = (BEQ, TCPS)
DISABLE_OOB = ON
EOF
    cat > "$admin/listener.ora" <<EOF
$name =
  (DESCRIPTION_LIST =
    (DESCRIPTION = (ADDRESS = (PROTOCOL = TCPS)(HOST = 0.0.0.0)(PORT = $port))))
SID_LIST_$name =
  (SID_LIST =
    (SID_DESC = (GLOBAL_DBNAME = $service)(ORACLE_HOME = $ORACLE_HOME)(SID_NAME = $sid)))
WALLET_LOCATION = (SOURCE = (METHOD = FILE)(METHOD_DATA = (DIRECTORY = $root/wallet)))
SSL_CLIENT_AUTHENTICATION = $client_auth
EOF
    TNS_ADMIN="$admin" lsnrctl stop "$name" > /dev/null 2>&1 || true
    TNS_ADMIN="$admin" lsnrctl start "$name" > "$admin/start.log" \
        || { cat "$admin/start.log"; exit 1; }
    echo "$name listening on $port (SSL_CLIENT_AUTHENTICATION = $client_auth)"
}

start RAJSKA_TLS 2484 FALSE
start RAJSKA_MTLS 2485 TRUE
