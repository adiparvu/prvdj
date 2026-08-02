#!/usr/bin/env bash
#
# Architecture rule validation.
#
# Master Prompt #28 requires architecture rules to be validated in continuous
# integration rather than left to review. Conventions that are only written down
# erode under deadline pressure and staff turnover; the ones checked here do not.
#
# Each rule below exists because breaking it would violate a specific commitment
# in the specification corpus, named in the failure message.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

failures=0

fail() {
    printf '  FAIL  %s\n' "$1" >&2
    failures=$((failures + 1))
}

pass() {
    printf '  ok    %s\n' "$1"
}

core_sources() {
    find core -path core/target -prune -o -name '*.rs' -print 2>/dev/null | sort
}

# The subset of the core that is actually linked into a shipping binary.
#
# Excludes `tests/`, `benches/`, `examples/` and `src/bin/`, none of which cargo
# links into a library. The distinction matters for Rule 1 and nowhere else: a
# test harness that spawns a compiler, or a code generator that writes a header,
# is not the core reaching for the filesystem — it is the build doing its job.
# Every other rule below still scans everything, because a placeholder marker or
# an undocumented crate is just as wrong in a test.
core_library_sources() {
    find core \
        -path core/target -prune -o \
        -path '*/tests/*' -prune -o \
        -path '*/benches/*' -prune -o \
        -path '*/examples/*' -prune -o \
        -path '*/src/bin/*' -prune -o \
        -name '*.rs' -print 2>/dev/null | sort
}

printf 'Architecture rules\n\n'

# ---------------------------------------------------------------------------
# Rule 1 — the core performs no input or output.
#
# ADR-0001 makes `prv-core` pure: everything the outside world provides enters
# through ports implemented by the host. A filesystem, network, process or
# environment call inside the core would bind it to a platform and break the
# Phase-2 targets of Master Prompt #8 before they are attempted.
# ---------------------------------------------------------------------------
io_hits=""
for forbidden in 'std::fs' 'std::net' 'std::process' 'std::env'; do
    hits="$(core_library_sources | xargs -r grep -l -- "$forbidden" || true)"
    if [ -n "$hits" ]; then
        io_hits="${io_hits}${forbidden}: ${hits}\n"
    fi
done
if [ -n "$io_hits" ]; then
    fail "the core must perform no I/O (ADR-0001); found:"
    printf '%b' "$io_hits" >&2
else
    pass "shipping core performs no filesystem, network, process or environment access"
fi

# ---------------------------------------------------------------------------
# Rule 2 — unsafe code is confined to audited crates.
#
# ADR-0002's guarantees rest on a small, reviewed set of wait-free structures.
# Unsafe code spreading beyond those crates would make the set impossible to
# audit, which is exactly the failure mode the confinement exists to prevent.
#
# Two crates are excepted, and both exceptions are the same shape: a place where
# Rust's guarantees genuinely end and the reasoning has to be done by hand.
#
#   prv-rt   the wait-free structures ADR-0002 rests on.
#   prv-ffi  the C boundary. A host hands over a pointer and a promise, and
#            there is no mechanism anywhere that can check the promise.
#
# The list is short on purpose. Adding to it is a decision, not a convenience,
# and it belongs in a pull request that says why.
#
# Comments are stripped before matching. The rule is about code, and a scanner
# that also matched prose would make documenting *why* a module avoids unsafe
# code into a build failure — which would train authors to stop explaining it.
# ---------------------------------------------------------------------------
unsafe_hits=""
while IFS= read -r source; do
    case "$source" in
        core/prv-rt/*) continue ;;
        core/prv-ffi/*) continue ;;
    esac
    if sed 's://.*::' "$source" | grep -q -E '\bunsafe\b'; then
        unsafe_hits="${unsafe_hits}${source}\n"
    fi
done < <(core_sources)
if [ -n "$unsafe_hits" ]; then
    fail "unsafe code is permitted only in prv-rt and prv-ffi (ADR-0002); found in:"
    printf '%b' "$unsafe_hits" >&2
else
    pass "unsafe code confined to prv-rt and prv-ffi"
fi

# ---------------------------------------------------------------------------
# Rule 3 — no placeholder implementations.
#
# Master Prompt #13: "No TODO comments. No placeholder methods." A marker left
# in the tree is a claim of completeness the code does not honour.
# ---------------------------------------------------------------------------
marker='TO''DO|FIX''ME|unimplemented!|todo!|XX''X'
placeholder_hits="$(core_sources | xargs -r grep -l -E "$marker" || true)"
if [ -n "$placeholder_hits" ]; then
    fail "placeholder markers are not permitted (Master Prompt #13); found in:"
    printf '%s\n' "$placeholder_hits" >&2
else
    pass "no placeholder markers in the core"
fi

# ---------------------------------------------------------------------------
# Rule 4 — every crate documents itself.
#
# Master Prompt #31: code without documentation is incomplete. The compiler
# enforces documentation on public items; this enforces it on the crate as a
# whole, which is where a new engineer starts reading.
# ---------------------------------------------------------------------------
undocumented=""
while IFS= read -r manifest; do
    crate_dir="$(dirname "$manifest")"
    lib="$crate_dir/src/lib.rs"
    if [ ! -f "$lib" ]; then
        continue
    fi
    if ! head -n 1 "$lib" | grep -q '^//!'; then
        undocumented="${undocumented}${lib}\n"
    fi
done < <(find core -maxdepth 2 -name Cargo.toml -not -path 'core/Cargo.toml' | sort)

if [ -n "$undocumented" ]; then
    fail "every crate must open with crate-level documentation (Master Prompt #31):"
    printf '%b' "$undocumented" >&2
else
    pass "every crate opens with crate-level documentation"
fi

# ---------------------------------------------------------------------------
# Rule 5 — the decision log is complete and its links resolve.
#
# Master Prompt #31 makes the decision log part of the specification. An index
# entry pointing at a missing record means the reasoning behind a decision has
# been lost, which is the thing the log exists to prevent.
# ---------------------------------------------------------------------------
broken_links=""
if [ -f docs/adr/README.md ]; then
    while IFS= read -r target; do
        if [ ! -f "docs/adr/$target" ]; then
            broken_links="${broken_links}${target}\n"
        fi
    done < <(grep -o '](\([0-9]\{4\}[^)]*\.md\))' docs/adr/README.md | sed 's/^](//; s/)$//' | sort -u)
fi
if [ -n "$broken_links" ]; then
    fail "the decision log references records that do not exist (Master Prompt #31):"
    printf '%b' "$broken_links" >&2
else
    pass "every decision record referenced by the index exists"
fi

# ---------------------------------------------------------------------------
# Rule 6 — every decision record carries a review date.
#
# Master Prompt #31 requires it. A decision without a review date is a decision
# nobody has agreed to re-examine.
# ---------------------------------------------------------------------------
missing_review=""
for record in docs/adr/[0-9][0-9][0-9][0-9]-*.md; do
    [ -e "$record" ] || continue
    if ! grep -q '^- Review date:' "$record"; then
        missing_review="${missing_review}${record}\n"
    fi
done
if [ -n "$missing_review" ]; then
    fail "every decision record must carry a review date (Master Prompt #31):"
    printf '%b' "$missing_review" >&2
else
    pass "every decision record carries a review date"
fi

# ---------------------------------------------------------------------------
# Rule 7 — no engine depends on entitlements.
#
# Master Prompt #29 promises that essential functionality is never artificially
# restricted, and that the product behaves identically at every tier. A promise
# like that decays: it survives the first release, and then somebody adds one
# tier check inside the mixer because it was the convenient place, and a year
# later nobody can say what the free tier does without reading the DSP.
#
# So the promise is structural. The engines cannot consult a licence because
# they cannot name the crate that holds one; entitlements are checked at the
# feature boundary, above all of them. This is the rule that keeps it true.
# ---------------------------------------------------------------------------
engine_crates="prv-time prv-rt prv-harmony prv-transport prv-dsp prv-waveform \
prv-project prv-library prv-analysis prv-mix prv-timeline prv-learning prv-export"
tainted=""
for engine in $engine_crates; do
    manifest="core/${engine}/Cargo.toml"
    [ -e "$manifest" ] || continue
    if grep -q 'prv-entitlements' "$manifest"; then
        tainted="${tainted}${manifest}\n"
    fi
done
if [ -n "$tainted" ]; then
    fail "no engine may depend on entitlements (Master Prompt #29):"
    printf '%b' "$tainted" >&2
else
    pass "no engine depends on entitlements"
fi

# ---------------------------------------------------------------------------
# Rule 8 — no credential material in the repository.
#
# Master Prompt #26: never hardcode API keys, access tokens, private keys,
# credentials or certificates. This is the rule most often kept by intention and
# broken by accident — a key pasted into a test to get a build green on a Friday
# outlives the Friday, and once it is in the history it is published whether or
# not the commit that removed it looks tidy.
#
# The patterns below are deliberately specific. A scanner that cries wolf is a
# scanner people learn to pass with `--no-verify`, and the useful property of
# this one is that a failure means something.
#
# It scans what git tracks or has staged, which is what is about to be published.
# A key in an untracked scratch file is the author's own business; a key that has
# been staged is one command away from being permanent.
# ---------------------------------------------------------------------------
scannable() {
    git ls-files -- \
        ':!:tools/check-architecture.sh' \
        ':!:*.lock' \
        2>/dev/null || true
}

credential_patterns=(
    # A private key of any kind, in its armoured form.
    '-----BEGIN [A-Z ]*PRIVATE KEY-----'
    # A certificate committed alongside code.
    '-----BEGIN CERTIFICATE-----'
    # An assignment of a named credential to a long literal. The length floor
    # keeps empty strings, placeholders and short identifiers out of it.
    '(api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token|client[_-]?secret|password)[[:space:]]*[:=][[:space:]]*.[A-Za-z0-9/+_-]{16,}'
    # Provider-shaped keys, which are recognisable on their own.
    'AKIA[0-9A-Z]{16}'
    'sk-[A-Za-z0-9]{32,}'
    'ghp_[A-Za-z0-9]{36}'
    'xox[baprs]-[A-Za-z0-9-]{10,}'
)

credential_hits=""
for pattern in "${credential_patterns[@]}"; do
    hits="$(scannable | xargs -r grep -l -E -- "$pattern" 2>/dev/null || true)"
    if [ -n "$hits" ]; then
        credential_hits="${credential_hits}${pattern}\n${hits}\n"
    fi
done
if [ -n "$credential_hits" ]; then
    fail "credential material must never be committed (Master Prompt #26); found:"
    printf '%b' "$credential_hits" >&2
else
    pass "no credential material in the repository"
fi

printf '\n'
# ---------------------------------------------------------------------------
# Rule 10 — the sandbox cannot reach the network.
#
# Master Prompt #26 promises that nothing leaves the device without an explicit
# agreement. Our own code honours that (`prv-security` decides, and the boundary
# reports it), but code can have bugs and an entitlement cannot: without
# `com.apple.security.network.client` the operating system makes an outbound
# connection impossible.
#
# So the absence of that key is a guarantee, and this rule is what keeps it
# absent. Cloud sync adds it in the same commit as the consent screen it depends
# on, and that commit changes this rule deliberately rather than quietly.
# ---------------------------------------------------------------------------
entitlements="apple/Resources/PRVStudio.entitlements"
if [ ! -f "$entitlements" ]; then
    fail "the entitlements file is missing: $entitlements"
# XML comments are stripped before matching, for the same reason Rule 2 strips
# `//`: this file *explains* why the network key is absent, and a scanner that
# also read the prose would make documenting the guarantee into a build failure.
# That would train authors to stop explaining it, which is the opposite of what
# the rule is for.
elif sed 's/<!--.*-->//; /<!--/,/-->/d' "$entitlements" \
    | grep -q "com.apple.security.network.client"; then
    fail "$entitlements grants network access; MP#26's promise is then only as good as our code"
elif ! grep -q "com.apple.security.app-sandbox" "$entitlements"; then
    fail "$entitlements does not enable the sandbox"
else
    pass "the sandbox cannot reach the network"
fi

# ---------------------------------------------------------------------------
# Rule 9 — the generated bindings match the boundary that generated them.
#
# The architecture overview requires bindings to be generated rather than
# hand-written, "because hand-written bindings drift". A drifting binding is a
# nastier defect than a broken one: it compiles, it links, it runs, and it
# computes the wrong answer. A header that still calls `PRV_PLAYBACK_PAUSED` 4
# after the library moved it to 5 shows the wrong thing on stage.
#
# The generator is the authority. This rule asserts the committed copy is what it
# produces today.
# ---------------------------------------------------------------------------
bridge_header="apple/PRVKit/Bridge/Generated/PRVBridge.h"
if [ ! -f "$bridge_header" ]; then
    fail "the generated C header is missing: $bridge_header"
elif ! command -v cargo >/dev/null 2>&1; then
    # A tree without a Rust toolchain can still be checked for everything else.
    # Saying so is better than a silent pass.
    pass "generated bindings not checked (no cargo on this machine)"
elif (cd core && cargo run -q -p prv-ffi --bin bridgegen -- --check "../$bridge_header" >/dev/null 2>&1); then
    pass "generated bindings match the boundary"
else
    fail "$bridge_header has drifted from prv-ffi; run: cd core && cargo run -p prv-ffi --bin bridgegen -- ../$bridge_header"
fi

if [ "$failures" -gt 0 ]; then
    printf '%d architecture rule(s) violated.\n' "$failures" >&2
    exit 1
fi
printf 'All architecture rules satisfied.\n'
