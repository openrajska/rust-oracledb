# The openrajska fork of rust-oracledb

This is [openrajska](https://github.com/openrajska)'s fork of
[`oracle/rust-oracledb`](https://github.com/oracle/rust-oracledb). It exists
for one consumer: `rajska_oracle`, the PHP extension in
[openrajska/php-oracle](https://github.com/openrajska/php-oracle) that hosts
the Rust thin Backend.

The fork carries a short list of contained patches that only add API, each
one offered upstream. The fork goes away when upstream has absorbed them all.
Why the fork exists, and the full list of patches with their exits, is in
[ADR 0002](https://github.com/openrajska/php-oracle/blob/main/docs/adr/0002-rust-thin-backend-is-the-primary-backend.md)
and in the spike's
[FINDINGS.md](https://github.com/openrajska/php-oracle/blob/main/spikes/rust-oracledb/FINDINGS.md)
(conditions C1–C21).

## The pin

| | |
|---|---|
| Upstream release | `oracledb` **26.0.0-beta.6** on crates.io |
| Upstream tag | [`v26.0.0-beta.6`](https://github.com/oracle/rust-oracledb/tree/v26.0.0-beta.6) = `f1a0d1890d13128c73ba26034d9da70677f3df6c` |
| Pin file | [`.rajska/upstream-tag`](.rajska/upstream-tag) |

The published crate was built from exactly that commit (its
`.cargo_vcs_info.json` names it, and the sources are identical). The fork
never follows upstream `main`: it moves only from one released tag to the
next, as a deliberate [upgrade](#upgrading-the-pin) (condition C17).

## Branches

| Branch | What it holds | Rewritten? |
|---|---|---|
| `rajska/base` | The upstream tag plus the fork's own files only: this document, the CI workflow, the pin file and the `Cargo.lock` shipped in the published crate. No behaviour change. | Only on an upgrade |
| `patch/<condition>-<slug>` | One patch, branched from `rajska/base`. Example: `patch/c21-autocommit`. | Only on an upgrade |
| `rajska/integration` | `rajska/base` plus a merge of every patch that has landed in the fork. **The Extension pins a commit of this branch.** Default branch. | Never: a ruleset blocks force-pushes and deletion |
| `main` | Mirror of upstream `main`, untouched. Upstream PR branches start here. | Follows upstream |

Because `rajska/integration` only moves forward, every commit the Extension
ever pinned stays reachable from it, across upgrades too. Changes to the
fork's own files are committed on `rajska/base` and then merged into
`rajska/integration`.

Tags:

- `integration-v<version>` marks the last `rajska/integration` commit built on
  upstream `<version>`, as a readable marker for the upgrade history.

## How the Extension depends on the fork

```toml
# packages/php-oracle-ext/Cargo.toml
[dependencies]
oracledb = { git = "https://github.com/openrajska/rust-oracledb", rev = "<full sha on rajska/integration>", version = "=26.0.0-beta.6" }
```

- `rev` is an exact commit of `rajska/integration`, never a branch name. A new
  patch reaches the Extension only when someone bumps `rev` in its own commit.
- `version = "=26.0.0-beta.6"` makes Cargo refuse the dependency if the fork
  ever reports another version, so the pin cannot drift silently.

## Patch workflow

Every patch has its own ticket in
[openrajska/php-oracle](https://github.com/openrajska/php-oracle/issues) and
follows the same steps.

1. **Branch.** Create `patch/<condition>-<slug>` from `rajska/base`, for
   example `patch/c21-autocommit` or `patch/c5-c6-bind-size`. One patch per
   branch; a patch that needs another one is branched from that patch's
   branch and says so in its description.
2. **Test first, against the real database.** Write the test that shows the
   missing behaviour in the crate's own suite (`tests/test_NNNN_*.rs`, in the
   upstream style, next to the related tests). It runs against Oracle AI
   Database 26ai Free and must fail before the change. Never mock the
   server.
3. **Change.** Make the smallest change that turns the test green. Add API;
   don't change existing behaviour. Keep upstream's style (`rustfmt.toml`)
   and update the doc comments of every API you add.
4. **CI.** Push the branch. The `rajska-ci` workflow runs the crate's whole
   suite, with the new test, against a `gvenzl/oracle-free` 26ai service. It
   must be green.
5. **Land in the fork.** Open a PR from the patch branch into
   `rajska/integration` and merge it with a merge commit (no squash, no
   rebase), so the patch's commits stay identifiable. CI runs again on
   `rajska/integration`.
6. **Offer it upstream.** See [Upstream PR](#upstream-pr). Record the
   upstream issue and PR links in the patch's ticket.
7. **Bump the Extension.** In php-oracle, a separate commit moves the
   Extension's `rev` to the new `rajska/integration` commit.

Fixing a patch after it has landed: push the fix to the same patch branch,
then repeat steps 4, 5 and 7.

### Upstream PR

Upstream develops in an internal repo and only accepts PRs from committers
who have signed the [Oracle Contributor Agreement](https://oca.opensource.oracle.com)
(see [CONTRIBUTING.md](CONTRIBUTING.md)).

1. Open an upstream issue describing the missing behaviour, if none exists.
2. Create `upstream/<condition>-<slug>` from upstream `main` (not from
   `rajska/base`, which carries the fork's own files) and cherry-pick the
   patch's commits onto it, resolving any drift.
3. Commit with `--signoff`, using the name and e-mail on the OCA, in
   upstream's commit style (a plain sentence, no Conventional Commits prefix).
4. Open the PR against `oracle/rust-oracledb:main`, referencing the issue,
   with the steps to validate it.

The fork keeps its patch regardless of the upstream outcome. A patch is
dropped only at an upgrade to a release that contains an equivalent change.

## Upgrading the pin

Upgrading is a deliberate change made in its own php-oracle ticket, never a
side effect. It reruns every patch's test.

1. Pick the new upstream **release tag** (never a `main` commit) and read its
   changelog (`doc/src/release_notes.rst`) for API changes and for patches
   upstream has absorbed.
2. Tag the current `rajska/integration` tip as `integration-v<old version>`
   and push the tag.
3. Rebuild `rajska/base`: reset it to the new tag, cherry-pick the fork's own
   commits, update `.rajska/upstream-tag`, and replace `Cargo.lock` with the
   one from the new published crate. Force-push.
4. Rebase each `patch/*` branch onto the new `rajska/base`. A patch that
   upstream absorbed is dropped: delete its branch, and keep its test only if
   upstream did not take one. Force-push every remaining branch and let CI run
   the whole suite on each of them.
5. Build the next integration on a fresh branch `rajska/next` from the new
   `rajska/base`, merging every remaining patch branch in condition order.
   Push it and wait for CI to be green on it: that run is every remaining
   patch's test against the new release.
6. Move `rajska/integration` forward without rewriting it. On `rajska/next`,
   run `git merge -s ours rajska/integration`, which records the old history
   as a parent but keeps `rajska/next`'s files. Then fast-forward
   `rajska/integration` to it, push, and delete `rajska/next`.
7. In php-oracle, bump `version = "=<new version>"` and `rev` together in one
   commit, then run the Extension's conformance suite and the spike's probes
   against it.

The CI guard keeps the pin honest. It fails when the crate version in
`Cargo.toml` and the tag in `.rajska/upstream-tag` disagree, or when that tag
is not an ancestor of the commit being tested. So the pin can't move by
merging upstream `main`, only by step 3.

## Running the tests

The suite needs an Oracle database and these environment variables (see
[tests/README.md](tests/README.md)):

| Variable | Value against 26ai Free |
|---|---|
| `RSO_TEST_CONNECT_STRING` | `localhost:1521/FREEPDB1` |
| `RSO_TEST_ADMIN_USER` / `RSO_TEST_ADMIN_PASSWORD` | `sys` / the container's `ORACLE_PASSWORD` |
| `RSO_TEST_MAIN_PASSWORD` | any password for the test user `rsotest` |
| `RSO_TEST_PROXY_PASSWORD` | any password for the proxy user `rsotestproxy` |
| `RSO_TEST_DIRECTORY_NAME` | optional; a DIRECTORY for the BFILE tests |
| `RUST_TEST_THREADS` | `1` (see below) |

```bash
cargo run --locked --bin create_test_schema   # drops and recreates rsotest
cargo test --locked --all-features
```

Run the tests one at a time. With Cargo's parallel default, the suite opens
and closes Sessions fast enough that the 26ai Free listener refuses some of
them with ORA-12516; serially, the whole suite takes about half a minute.

From the php-oracle monorepo, with this repository cloned next to it
(`../rust-oracledb`), `ddev fork` runs `cargo` in this repository inside the
web container, with these variables set against ddev's Oracle service:

```bash
ddev fork run --locked --bin create_test_schema
ddev fork test --locked --all-features
```

CI ([`.github/workflows/rajska-ci.yml`](.github/workflows/rajska-ci.yml)) runs
the same commands on every push to `rajska/**` and `patch/**` and on every pull
request, against the `gvenzl/oracle-free` image the monorepo's ddev uses.
