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
    hits="$(core_sources | xargs -r grep -l -- "$forbidden" || true)"
    if [ -n "$hits" ]; then
        io_hits="${io_hits}${forbidden}: ${hits}\n"
    fi
done
if [ -n "$io_hits" ]; then
    fail "the core must perform no I/O (ADR-0001); found:"
    printf '%b' "$io_hits" >&2
else
    pass "core performs no filesystem, network, process or environment access"
fi

# ---------------------------------------------------------------------------
# Rule 2 — unsafe code is confined to one audited crate.
#
# ADR-0002's guarantees rest on a small, reviewed set of wait-free structures.
# Unsafe code spreading beyond `prv-rt` would make that set impossible to audit,
# which is exactly the failure mode the confinement exists to prevent.
# ---------------------------------------------------------------------------
unsafe_hits="$(core_sources | grep -v '^core/prv-rt/' | xargs -r grep -l -E '\bunsafe\b' || true)"
if [ -n "$unsafe_hits" ]; then
    fail "unsafe code is permitted only in prv-rt (ADR-0002); found in:"
    printf '%s\n' "$unsafe_hits" >&2
else
    pass "unsafe code confined to prv-rt"
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

printf '\n'
if [ "$failures" -gt 0 ]; then
    printf '%d architecture rule(s) violated.\n' "$failures" >&2
    exit 1
fi
printf 'All architecture rules satisfied.\n'
