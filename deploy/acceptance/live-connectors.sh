#!/usr/bin/env bash
# Live connector checks: the Forge PostgreSQL and S3 connectors against real servers,
# which the in-process fakes in forge/connector-sdk/tests cannot stand in for, and S3
# through the egress broker to a TLS server. [FR-F-2] [NFR-SEC-1] [NFR-PRV-1]
#
# Starts PostgreSQL 16 (host connections SCRAM-SHA-256 only), MinIO, and a second MinIO
# serving TLS with a certificate from a throwaway CA, on a private Docker network. Seeds
# a table and the buckets, renders the egress broker's configuration with forge-egress
# (one route pinned to the right CA, one to an unrelated CA), checks it with
# `haproxy -c`, runs the broker, and runs forge/connector-sdk/tests/live.rs in the
# pinned toolchain container on that network. Nothing is published on a host port, and
# everything it starts is removed on exit.
#
#   bash deploy/acceptance/live-connectors.sh
#
# Needs the images postgres:16 (also used for openssl), minio/minio, minio/mc, the
# pinned HAProxy below, and the toolchain image midgard-acceptance:1.82.0. Kafka is not
# covered: no broker image is assumed. PostgreSQL does not go through the broker: it
# negotiates TLS inside its own protocol, which a TLS-originating proxy cannot speak
# before PostgreSQL 17's direct TLS.
set -euo pipefail
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
IMAGE="${MIDGARD_TOOLCHAIN:-midgard-acceptance:1.82.0}"
HAPROXY=haproxy@sha256:34cc7d1f6142464d7d2b73e2a1eef7392556dbf304160aef543e513cfd9e5162
NET=midgard-live-connectors
PG=midgard-live-pg
MINIO=midgard-live-minio
MINIO_TLS=midgard-live-minio-tls
BROKER=midgard-live-broker
CERTS=midgard-live-certs
BROKER_CFG=midgard-live-broker-cfg

cleanup() {
    docker rm -f "$PG" "$MINIO" "$MINIO_TLS" "$BROKER" >/dev/null 2>&1 || true
    docker volume rm -f "$CERTS" "$BROKER_CFG" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup
docker network create "$NET" >/dev/null

docker run -d --name "$PG" --network "$NET" \
    -e POSTGRES_PASSWORD=live-admin-pass \
    -e POSTGRES_DB=fleet \
    -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
    -e POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256 \
    postgres:16 >/dev/null
docker run -d --name "$MINIO" --network "$NET" \
    -e MINIO_ROOT_USER=forge-reader \
    -e MINIO_ROOT_PASSWORD=live-minio-secret \
    minio/minio server /data >/dev/null

echo "--- a throwaway CA, a server certificate for $MINIO_TLS, and an unrelated CA"
docker volume create "$CERTS" >/dev/null
docker run --rm -v "$CERTS:/certs" --entrypoint sh postgres:16 -c "
    set -e; cd /certs
    openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj '/CN=midgard live CA' \
        -keyout ca.key -out ca.pem 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -subj '/CN=$MINIO_TLS' \
        -keyout private.key -out server.csr 2>/dev/null
    printf 'subjectAltName=DNS:$MINIO_TLS\n' > san.ext
    openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -days 1 \
        -extfile san.ext -out public.crt 2>/dev/null
    openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj '/CN=someone else' \
        -keyout other.key -out other-ca.pem 2>/dev/null
    chmod 0644 ./*"
docker run -d --name "$MINIO_TLS" --network "$NET" \
    -e MINIO_ROOT_USER=forge-reader \
    -e MINIO_ROOT_PASSWORD=live-minio-secret \
    -v "$CERTS:/certs:ro" \
    minio/minio server /data --certs-dir /certs >/dev/null

echo "--- seeding PostgreSQL"
for _ in $(seq 1 60); do
    docker exec "$PG" pg_isready -U postgres -d fleet >/dev/null 2>&1 && break
    sleep 1
done
sleep 2
docker exec -i "$PG" psql -v ON_ERROR_STOP=1 -U postgres -d fleet >/dev/null <<'SQL'
CREATE TABLE aircraft (id bigint PRIMARY KEY, name text NOT NULL, active boolean NOT NULL,
                       score double precision);
INSERT INTO aircraft VALUES (1, 'alpha, north', true, 1.5), (2, 'beta', false, NULL),
                            (3, 'gamma', true, 7);
CREATE ROLE reader LOGIN PASSWORD 'live-reader-pass';
GRANT SELECT, INSERT ON aircraft TO reader;
SQL

echo "--- seeding MinIO"
seeded=""
for _ in $(seq 1 30); do
    if docker run --rm --network "$NET" --entrypoint sh minio/mc -c '
        mc alias set live http://midgard-live-minio:9000 forge-reader live-minio-secret >/dev/null &&
        mc mb --ignore-existing live/fleet-exports >/dev/null &&
        printf "tail,hours,note\nMG-101,6412,\"based north, active\"\n" | mc pipe live/fleet-exports/aircraft/2026-09-01.csv >/dev/null &&
        printf "tail,hours,note\r\nMG-102,,held\r\nMG-103,5,\"two\nlines\"\r\n" | mc pipe live/fleet-exports/aircraft/2026-09-02.csv >/dev/null
    '; then
        seeded=yes
        break
    fi
    sleep 1
done
[ -n "$seeded" ] || { echo "MinIO did not come up" >&2; exit 1; }

echo "--- seeding the TLS MinIO"
seeded=""
for _ in $(seq 1 30); do
    if docker run --rm --network "$NET" --entrypoint sh minio/mc -c "
        mc alias set tls https://$MINIO_TLS:9000 forge-reader live-minio-secret --insecure >/dev/null &&
        mc mb --insecure --ignore-existing tls/fleet-exports >/dev/null &&
        printf 'tail,hours,note\nMG-201,1,a\nMG-202,2,b\nMG-203,3,c\n' | mc pipe --insecure tls/fleet-exports/aircraft/tls.csv >/dev/null
    "; then
        seeded=yes
        break
    fi
    sleep 1
done
[ -n "$seeded" ] || { echo "the TLS MinIO did not come up" >&2; exit 1; }

echo "--- syncing the tree into the verification volume"
docker run --rm -v midgard-src-verify:/work "$IMAGE" \
    find /work -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} +
tar -C . --exclude=./target --exclude=node_modules -cf - . |
    docker run --rm -i -v midgard-src-verify:/work "$IMAGE" tar -xf - -C /work

echo "--- rendering and checking the egress broker's configuration"
docker volume create "$BROKER_CFG" >/dev/null
docker run --rm -i -v "$BROKER_CFG:/cfg" "$IMAGE" sh -c 'cat > /cfg/routes.json' <<JSON
{ "version": 1,
  "routes": [
    { "name": "exports", "listen": 19000,
      "upstream": { "host": "$MINIO_TLS", "port": 9000 }, "ca": "ca.pem" },
    { "name": "impostor", "listen": 19001,
      "upstream": { "host": "$MINIO_TLS", "port": 9000 }, "ca": "other-ca.pem" }
  ] }
JSON
docker run --rm --network none \
    -v midgard-src-verify:/work -v midgard-target:/work/target -v "$BROKER_CFG:/cfg" -w /work \
    "$IMAGE" cargo run --offline -q -p forge-worker --bin forge-egress -- \
        --routes /cfg/routes.json --bind 0.0.0.0 --ca-dir /etc/egress/ca --out /cfg/haproxy.cfg
docker run --rm --network "$NET" -v "$BROKER_CFG:/cfg:ro" -v "$CERTS:/etc/egress/ca:ro" \
    "$HAPROXY" haproxy -c -f /cfg/haproxy.cfg
docker run -d --name "$BROKER" --network "$NET" \
    -v "$BROKER_CFG:/cfg:ro" -v "$CERTS:/etc/egress/ca:ro" \
    "$HAPROXY" haproxy -f /cfg/haproxy.cfg >/dev/null
sleep 1

echo "--- running the live connector checks"
output="$(docker run --rm --network "$NET" \
    -v midgard-src-verify:/work -v midgard-target:/work/target -w /work \
    -e MIDGARD_LIVE_CONNECTORS=1 \
    -e MIDGARD_LIVE_PG="$PG:5432" \
    -e MIDGARD_LIVE_S3="$MINIO:9000" \
    -e MIDGARD_LIVE_S3_TLS="$MINIO_TLS:9000" \
    -e MIDGARD_LIVE_BROKER="$BROKER:19000" \
    -e MIDGARD_LIVE_BROKER_IMPOSTOR="$BROKER:19001" \
    "$IMAGE" cargo test --offline -p forge-connector-sdk --test live -- --nocapture 2>&1)" || {
    echo "$output"
    docker logs "$BROKER" 2>&1 | tail -20
    exit 1
}
echo "$output" | grep -E 'LIVE|test result|panicked'
docker logs "$BROKER" 2>&1 | tail -6
grep -q 'LIVE postgres: PASS' <<<"$output"
grep -q 'LIVE s3: PASS' <<<"$output"
grep -q 'LIVE broker: PASS' <<<"$output"
echo "live connectors: PASS"
