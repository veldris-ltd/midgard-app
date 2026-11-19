#!/usr/bin/env bash
# Every function and constructor the generated Swift bindings export is used by
# ContractTests.swift (the key root's callbacks: implemented by TestKeyRoot.swift).
# [FR-C-1] [DD-6]
#
#   bash clients/shells/apple/tools/check-contract.sh
set -euo pipefail
cd "$(dirname "$0")/.."
gen=Sources/MidgardCore/midgard_client_ffi.swift
test=Tests/MidgardKitTests/ContractTests.swift
keyroot=Tests/MidgardKitTests/TestKeyRoot.swift
[ -f "$gen" ] || {
    echo "check-contract: no $gen (run tools/build-xcframework.sh first)" >&2
    exit 2
}
missing=0
count=0
# Object methods (`open func name(`) and top-level functions (`public func name(`),
# minus UniFFI's own converters and helpers.
names="$(grep -Eo '(open|public) func [a-zA-Z0-9_]+\(' "$gen" | sed -E 's/.* func ([a-zA-Z0-9_]+)\(/\1/' |
    grep -Ev '^(FfiConverter|uniffi|lift|lower|read|write|allocationSize|hash)' | sort -u)"
[ -n "$names" ] || {
    echo "check-contract: no bindings found in $gen" >&2
    exit 2
}
for name in $names; do
    count=$((count + 1))
    if ! grep -q "[.( ]$name(" "$test" "$keyroot"; then
        echo "check-contract: binding $name is not used in $test"
        missing=$((missing + 1))
    fi
done
# Constructors of Rust objects (`public convenience init(label:`), called as `Type(label:`.
classes="$(grep -Eo '^open class [A-Za-z0-9_]+' "$gen" | sed -E 's/^open class //' | grep -v 'Impl$' || true)"
for cls in $classes; do
    labels="$(awk -v c="open class $cls" '
        index($0, c) == 1 { inside = 1; next }
        inside && /^open class / { inside = 0 }
        inside && /public convenience init\(/ { sub(/.*public convenience init\(/, ""); sub(/:.*/, ""); print }
    ' "$gen")"
    for label in $labels; do
        count=$((count + 1))
        if ! grep -q "$cls($label:" "$test"; then
            echo "check-contract: constructor $cls($label:) is not called in $test"
            missing=$((missing + 1))
        fi
    done
done
if [ "$missing" -eq 0 ]; then
    echo "check-contract: every binding is exercised ($count)"
fi
[ "$missing" -eq 0 ]
