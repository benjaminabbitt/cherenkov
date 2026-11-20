# Building

The run layer's messages (commands, facts, events, refusals, snapshots and
the documents a run prints) are defined in `proto/cherenkov/` and generated
at build time by angzarr-build into `OUT_DIR`, never committed:

- `cherenkov-run-wire`'s build script runs `angzarr_build::wire` (the prost
  types) and `angzarr_build::json` (proto3 JSON as `serde` impls, written
  from the `io.angzarr.codegen.v1` options in the protos: snake_case keys,
  kebab-case enum names, explicit nulls, flattened fields and the `event`
  discriminator of `cherenkov.output.v1`'s `Document`);
- `cherenkov-run-domain`'s build script runs `angzarr_build::markup_domain_with`
  (the domain types: the layer of each package at `cherenkov_run_domain::run`
  and `::mutant`, the event enums the streams are read as, the refusals'
  `Refusal` statuses, the `RunId` newtype of a run's UUID).

No message is a hand-written `#[derive(prost::Message)]` or `DomainMessage`
impl; a new message, or a change to one, is a change to the `.proto` files.
The documents of `docs/output-events.md` are the messages of
`cherenkov.output.v1`: change a document there, not in code.

The plan of a mutant (`plan.proto`) and what an executor observed have
domain types too; the planner's own types in `cherenkov-core` convert to
and from them in `plan::encode` and `plan::decode`, `observation::encode`
and `observation::decode`. The generated JSON covers the output package only
(the generated readers of the plan's messages pass the complexity gate's
line limit).

Building needs `protoc` on `PATH`, or named by `$PROTOC`: angzarr-build runs
it and bundles no compiler. It also needs access to github.com for the git
dependencies (angzarr's protos come from angzarr-proto, never a vendored
copy).

A message's field numbers, names, types and package are its wire form:
the `encodings.txt` beside `cherenkov-run-wire`'s and `cherenkov-run-domain`'s
tests holds every message's and refusal's bytes, and those tests fail on any
change to them.

The angzarr pins are one full commit, `71b453a` of github.com/angzarr-io/angzarr
(core, embedded, router, build and platform are crates of that one workspace),
and rust-support (cli-support) is on the same one, so the lock holds one core. A change to what an aggregate's state holds or how its events
fold bumps `cherenkov-run-domain`'s minor version: it is the rules version
under which snapshots are kept.

# Hooks and CI

Secrets are scanned with gitleaks in a pinned container (`just scan-secrets`
on the staged files, `just scan-secrets-all` on the history; a false positive
goes in `.gitleaksignore` or `.gitleaks.toml`, never `--no-verify`). Hooks are
lefthook's (`lefthook.yml`): run `lefthook install` once per clone. Pre-commit
is fast checks only (secrets, fmt, clippy); pre-push runs `just check`, red
tests allowed off master with a warning. CI (`.github/workflows/ci.yml`)
installs tools and calls just recipes only; actions are pinned to SHAs.

# Co-developing the angzarr backends

The angzarr git dependencies are pinned by full commit in `Cargo.toml`. To
build against the backends' working trees instead, link the shared dev module
(rust-support's `tools/dev/install.sh <this checkout>`, once per clone; `mod? dev '.dev'` in the
justfile, `/.dev` gitignored) and run `just dev::local`: it writes the
gitignored `.cargo/config.toml` `[patch]`, one path per crate, found through
`~/.config/angzarr-dev/repos.toml` (name or URL to checkout) or the usual
places under `~/workspace`. `just dev::local-off` undoes it and restores
`Cargo.lock`, which cargo rewrites while the patch is on: never commit it
that way. `just dev::doctor` says what was found. After a backend commit,
`just dev::repin` sets the pins to its HEAD (`cargo update -p` for those only),
`just dev::land` prints the push order across repositories (`--go` pushes, each
repository's own hooks run), `just dev::pins-check` (pre-commit) and `just
dev::pins-reachable` (pre-push) refuse a path-based commit and a pin that is
not on its remote. `pins.toml` holds what is specific to this repository.
Nothing path-based is ever committed.

# Complexity lints

`cognitive_complexity`, `too_many_lines` and `excessive_nesting` (threshold 6, `clippy.toml`) are `warn` in `[workspace.lints.clippy]` and so fail `just lint` (`-D warnings`), hence `just check`, the hooks and CI. A warning is fixed by refactoring into named helpers, never by `#[allow]`, `#[expect]` or a raised threshold.
