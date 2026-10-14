#!/usr/bin/env bash
# Run the acceptance gate and keep what it printed. [AC-RF-CA-5a] [AC-RF-CA-5d] [AC-M-5]
#
#   bash tools/ci/gate-run.sh                 # both parts
#   bash tools/ci/gate-run.sh acceptance      # deploy/acceptance/run-airgapped.sh
#   bash tools/ci/gate-run.sh test            # just test
#   bash tools/ci/gate-run.sh --out DIR ...   # somewhere other than target/gate
#
# The gate is two runs in the pinned toolchain image under `docker run --network none`:
# the acceptance suite, which gives each criterion its verdict, and `just test`, which
# runs every crate's tests under both crypto backends. This runs them and writes, for
# each part,
#
#   <out>/<part>.log    everything the run printed, unedited
#   <out>/<part>.meta   what the log cannot say for itself: the commit, whether the
#                       tree was clean, the image, the toolchain, the times, the exit
#
# tools/ci/gate-record.sh turns those into the record under docs/compliance/gate-runs/.
# Nothing is judged here: a failing run still leaves its log and its metadata, because
# a failed gate is recorded too. The exit status is the run's own.
set -uo pipefail

cd "$(dirname "$0")/../.."
. tools/ci/lib.sh

IMAGE="${TOOLCHAIN_IMAGE:-${IMAGE:-midgard-acceptance:1.82.0}}"
DOCKERFILE="deploy/acceptance/Dockerfile"
out="target/gate"
parts=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) out="${2:?--out needs a directory}"; shift 2 ;;
        acceptance | test) parts+=("$1"); shift ;;
        -h | --help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "gate-run: unknown argument '$1'" >&2; exit 2 ;;
    esac
done
[ "${#parts[@]}" -gt 0 ] || parts=(acceptance test)

command -v docker >/dev/null 2>&1 || { echo "gate-run: docker is not available" >&2; exit 2; }
mkdir -p "$out"

sha256_of() { (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'; }
now() { date -u +%Y-%m-%dT%H:%M:%SZ; }

# The commit a CI system checked out, else the checkout's own. A pull request's merge
# commit is not in anyone's history, so a record cannot be made from one; gate-record.sh
# is where that is refused.
commit="${MIDGARD_SOURCE_REVISION:-$(git rev-parse HEAD)}"
# target/ and the other ignored paths are not part of the tree being gated.
if [ -z "$(git status --porcelain 2>/dev/null)" ]; then tree=clean; else tree=dirty; fi

# The FROM line pins the base image by digest: that, and the Dockerfile itself, are what
# a third party rebuilds from. The built image's id is recorded too, but it is local to
# the machine that built it.
base_image="$(awk 'toupper($1) == "FROM" { print $2; exit }' "$DOCKERFILE")"
dockerfile_sha="$(sha256_of "$DOCKERFILE")"

run_part() {
    local part="$1" log="$out/$1.log" meta="$out/$1.meta"
    local started finished t0 t1 rc image_id rustc cargo
    rm -f "$log" "$meta"
    started="$(now)"
    t0="$(date +%s)"
    case "$part" in
        acceptance)
            # Builds the image if it is missing, then runs every verify-*.sh under
            # --network none.
            IMAGE="$IMAGE" bash deploy/acceptance/run-airgapped.sh 2>&1 | tee "$log"
            rc="${PIPESTATUS[0]}"
            ;;
        test)
            # The command line is in the log because `just` echoes each recipe line; the
            # record reads the invocations from there.
            TOOLCHAIN_IMAGE="$IMAGE" bash tools/ci/in-toolchain.sh just test 2>&1 | tee "$log"
            rc="${PIPESTATUS[0]}"
            ;;
    esac
    t1="$(date +%s)"
    finished="$(now)"
    image_id="$(docker image inspect --format '{{.Id}}' "$IMAGE" 2>/dev/null || true)"
    rustc="$(docker run --rm --network none "$IMAGE" rustc --version 2>/dev/null | tr -d '\r' || true)"
    cargo="$(docker run --rm --network none "$IMAGE" cargo --version 2>/dev/null | tr -d '\r' || true)"
    {
        echo "part=$part"
        echo "commit=$commit"
        echo "tree=$tree"
        echo "image=$IMAGE"
        echo "image_id=$image_id"
        echo "base_image=$base_image"
        echo "dockerfile_sha256=$dockerfile_sha"
        echo "rustc=$rustc"
        echo "cargo=$cargo"
        echo "network=none"
        echo "started=$started"
        echo "finished=$finished"
        echo "seconds=$((t1 - t0))"
        echo "exit=$rc"
        echo "platform=$(ci_platform)"
        echo "run_url=$(ci_run_url)"
    } > "$meta"
    echo "gate-run: $part exited $rc after $((t1 - t0))s; log and metadata in $out/"
    return "$rc"
}

status=0
for part in "${parts[@]}"; do
    run_part "$part" || status=1
done
exit "$status"
