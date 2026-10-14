# Gate runs

A gate run is one run of the acceptance gate at one commit: the acceptance suite
(`deploy/acceptance/run-airgapped.sh`) and `just test`, both in the pinned toolchain
image under `docker run --network none`. This directory holds the record of each run
that a published figure rests on.

| Path | What it is |
|---|---|
| `CURRENT` | the commit of the record now in force |
| `<commit>.md` | the record: the result for every acceptance criterion, every test invocation and its counts, the toolchain, the times, and the SHA-256 of each log |
| `<commit>/acceptance.log`, `<commit>/test.log` | everything the two runs printed, unedited |
| `<commit>/acceptance.meta`, `<commit>/test.meta` | what a log cannot say for itself: the commit, that the tree was clean, the image, the exit status |

Nothing here is written by hand. [`tools/ci/gate-record.sh`](../../../tools/ci/gate-record.sh)
makes the record from the logs and metadata beside it, together with two files as they
stood at the gated commit: Part C of the acceptance spec, which names the criteria, and
the justfile's `test` recipe, which names the test invocations. CI makes the record
again on every change and requires the same bytes, so a record cannot be edited and a
log cannot be swapped. The status tables in the README and in `GA_READINESS.md` are
generated from the current record and checked the same way.

## What a record does and does not say

- **It covers one commit.** The record names the commit that was gated, so it cannot be
  part of that commit; it is committed after it. Later commits are not covered until a
  new record is made. A release must be of the gated commit apart from the record and
  the prose generated from it, and `gate-record.sh --check --release` enforces that.
- **Every criterion has a verdict, or there is no record.** A run whose log lacks a
  verdict for any criterion of the spec is refused, as is one that gives a verdict for
  a criterion the spec does not define.
- **A failure is recorded.** A run in which a criterion fails still makes a record. It
  says FAIL, names the criterion, and the README says the same.
- **Some criteria are verified together.** Where one step of the suite covers several
  criteria, its verdict line names them all (`AC-B-1/4`), and the record gives each of
  them that step's result and shows the verdict line it came from.
- **Counts are of tests that ran.** A test count is the sum of the `test result:` lines
  in a log. It is not the number of test functions in the source, and the acceptance
  suite's count is not comparable with the `just test` count: the suite runs selected
  tests, some of them more than once.

## Checking a record

```bash
bash tools/ci/gate-record.sh --check        # the record is what its logs give
sha256sum docs/compliance/gate-runs/<commit>/*.log   # the digests the record cites
```

## Reproducing a run

```bash
git checkout <commit>
bash tools/ci/gate-run.sh                   # builds the toolchain image once, then runs air-gapped
bash tools/ci/gate-record.sh --compare target/gate
```

`--compare` holds the fresh run's result for each criterion, and its test totals,
against the record. Timings and the locally built image's id will differ.
