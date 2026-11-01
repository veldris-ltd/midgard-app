# Shell-audit tooling

The `shell-audit` job (`tools/ci/run.sh`) serves a built web or extension shell with
`http-server` and waits for it with `wait-on` before the axe audit runs. The record that
audit writes gates the release, so both tools are pinned here, to exact versions, in a
lockfile that carries the registry's integrity hash for every package. The job installs
them with `npm ci --ignore-scripts` and runs them from `node_modules/.bin`; nothing is
fetched by name at run time, and `tools/ci/tests/npx-pin.test.sh` refuses an unpinned
`npx` anywhere in the CI tooling (VLD-ANB-MIDGARD-0058). To bump a pin, change the exact
version in `package.json` (no caret or tilde), regenerate the lockfile with
`npm install --package-lock-only --ignore-scripts`, check that the integrity of each
direct dependency in `package-lock.json` matches `npm view <name>@<version>
dist.integrity`, and review the transitive changes in the diff before committing both
files together.
