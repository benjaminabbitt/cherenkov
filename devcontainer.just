# Dev container and run-measurement recipes, imported by the justfile
# (`import 'devcontainer.just'`). The container's definition is in
# .devcontainer/ (Containerfile, devcontainer.json). CI calls just recipes,
# not tool commands, so these recipes are what a contributor and CI share.

# The image the recipes build and enter.
DEVCONTAINER_IMAGE := env_var_or_default("CHERENKOV_DEVCONTAINER_IMAGE", "cherenkov-dev:local")

# Build the dev container image (docker if its daemon answers, else podman).
devcontainer-build:
    #!/usr/bin/env bash
    set -euo pipefail
    if docker info >/dev/null 2>&1; then rt=docker; else rt=podman; fi
    "$rt" build -f .devcontainer/Containerfile -t "{{DEVCONTAINER_IMAGE}}" .devcontainer

# Enter the dev container with this checkout mounted at /workspaces/cherenkov.
# ptrace is enabled for GDB and Delve, and perfmon for perf and samply (see
# .devcontainer/devcontainer.json).
devcontainer-shell:
    #!/usr/bin/env bash
    set -euo pipefail
    if docker info >/dev/null 2>&1; then rt=docker; else rt=podman; fi
    "$rt" run --rm -it --cap-add=SYS_PTRACE --cap-add=PERFMON --security-opt=seccomp=unconfined \
        --userns=keep-id -v "{{justfile_directory()}}:/workspaces/cherenkov" \
        -w /workspaces/cherenkov "{{DEVCONTAINER_IMAGE}}" bash

# The dev container's postCreateCommand: install the git hooks and print the
# toolchain, so a broken container shows at creation.
devcontainer-setup:
    #!/usr/bin/env bash
    set -euo pipefail
    git config --global --add safe.directory '*'
    if [ -d .git ] || [ -f .git ]; then lefthook install; fi
    rustc --version; cargo --version; gdb --version | head -n1; protoc --version
    just --version; lefthook version; gitleaks version; cargo mutants --version
    dlv version | head -n1; python3 -c 'import debugpy; print("debugpy", debugpy.__version__)'

# Run COMMAND while measuring it (wall time, max RSS, peak extra disk of the
# watched directories) and print a JSON block for a dogfood record.
# e.g. `just measure-run target:/tmp cherenkov run .`
measure-run WATCH +COMMAND:
    scripts/measure-run.sh "{{WATCH}}" {{COMMAND}}
