#!/usr/bin/env bash
set -euo pipefail

profile="debug"
cargo_args=()
for arg in "$@"; do
    if [[ "$arg" == "--release" ]]; then
        profile="release"
    fi
    cargo_args+=("$arg")
done

if (( ${#cargo_args[@]} > 0 )); then
    cargo build --bin space_query "${cargo_args[@]}"
else
    cargo build --bin space_query
fi

src_bin="$(find target -type f -path "*/${profile}/space_query" | head -n 1 || true)"
if [[ -z "${src_bin}" ]]; then
    src_bin="target/${profile}/space_query"
fi
bin_dir="$(dirname "${src_bin}")"

if [[ ! -f "${src_bin}" ]]; then
    echo "Unable to find built binary: ${src_bin}" >&2
    exit 1
fi

dst_bin="${bin_dir}/SPACE Query"
cp -f "${src_bin}" "${dst_bin}"
chmod +x "${dst_bin}"

echo "Built executable:"
echo "  ${dst_bin}"
