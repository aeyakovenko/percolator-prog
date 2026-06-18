#!/usr/bin/env python3
"""Wrapper-side engine contract gate.

This is deliberately a wrapper boundary check. The engine proves accounting,
rank, conservation, and arithmetic under its named axioms; the wrapper only
proves/guards that it is pinned to the certified engine surface, routes the
public crank through the engine-selected auto-crank, and keeps wrapper Kani
proofs focused on serialization / matcher ABI boundaries instead of duplicating
engine internals.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CERTIFIED_ENGINE_REV = "a64e0fd7b6b67a844a311e061c0da94ee5804821"
MANIFEST = ROOT / "audits" / "engine_contracts.json"


def read(path: str) -> str:
    return (ROOT / path).read_text()


def fail(gaps: list[str], msg: str) -> None:
    gaps.append(msg)


def cargo_revs() -> set[str]:
    text = read("Cargo.toml") + "\n" + read("Cargo.lock")
    revs = set(re.findall(r"percolator[^\n]*rev=([0-9a-f]{40})", text))
    revs.update(re.findall(r"percolator[^\n]*rev = \"([0-9a-f]{40})\"", text))
    return revs


def main() -> int:
    gaps: list[str] = []

    if not MANIFEST.exists():
        fail(gaps, f"missing imported engine contract manifest: {MANIFEST.relative_to(ROOT)}")
        manifest = {}
    else:
        manifest = json.loads(MANIFEST.read_text())

    revs = cargo_revs()
    if revs != {CERTIFIED_ENGINE_REV}:
        fail(
            gaps,
            "wrapper percolator pin must match certified engine rev "
            f"{CERTIFIED_ENGINE_REV}; found {sorted(revs)}",
        )

    contracts = manifest.get("contracts", {})
    auto = contracts.get("permissionless_auto_crank_not_atomic")
    if auto is None:
        fail(gaps, "engine manifest must export permissionless_auto_crank_not_atomic")
    else:
        if auto.get("class_tier") != "THEOREM":
            fail(gaps, "auto-crank contract must be class_tier=THEOREM")
        if auto.get("liveness") != "dispatcher(select_auto_crank_plan)":
            fail(gaps, "auto-crank contract must be the selector dispatcher")

    if "permissionless_crank_not_atomic" in contracts:
        fail(gaps, "old direct permissionless_crank_not_atomic must not be public in manifest")

    assumptions = set(manifest.get("assumptions", []))
    expected_assumption = (
        "Wrapper proves account routing / auth / oracle freshness / engine Err propagation boundaries"
    )
    if expected_assumption not in assumptions:
        fail(gaps, "manifest must name wrapper routing/auth/oracle/error-propagation obligations")

    src = read("src/v16_program.rs")
    if src.count(".permissionless_auto_crank_not_atomic(") != 1:
        fail(gaps, "wrapper must call exactly one engine auto-crank entrypoint")
    if ".permissionless_crank_not_atomic(" in src:
        fail(gaps, "wrapper must not call the old direct engine crank")
    if "Instruction::PermissionlessCrank" not in src:
        fail(gaps, "wrapper public PermissionlessCrank instruction dispatch is missing")
    if "handle_close_resolved(program_id, accounts" not in src:
        fail(gaps, "resolved payout route must remain explicit wrapper terminal handling")
    auto_pos = src.find(".permissionless_auto_crank_not_atomic(")
    if auto_pos >= 0:
        auto_stmt_end = src.find(";", auto_pos)
        auto_stmt = src[auto_pos:auto_stmt_end if auto_stmt_end >= 0 else len(src)]
        if ".map_err(map_v16_error)?" not in auto_stmt:
            fail(gaps, "engine auto-crank Err must be mapped and propagated with ?")
    if re.search(r"\.map_err\(map_v16_error\)\s*;", src):
        fail(gaps, "engine Err mapped with map_v16_error must not be dropped")

    kani = read("tests/v16_kani.rs")
    duplicate_markers = [
        "percolator::",
        "use percolator::",
        "extern crate percolator",
        "GlobalValidState",
        "validate_shape",
        "loss_weight",
        "_not_atomic",
    ]
    for marker in duplicate_markers:
        if marker in kani:
            fail(
                gaps,
                "wrapper Kani must stay on wrapper serialization/matcher boundaries; "
                f"found engine-internal marker {marker!r}",
            )

    if gaps:
        print("WRAPPER CONTRACT GATE GAP(S):")
        for gap in gaps:
            print(f"  - {gap}")
        return 1

    print("wrapper contract gate OK:")
    print(f"  engine rev: {CERTIFIED_ENGINE_REV}")
    print("  imported manifest: audits/engine_contracts.json")
    print("  public crank route: wrapper PermissionlessCrank -> engine auto-crank")
    print("  engine Err propagation: mapped engine errors are returned to SVM")
    print("  wrapper Kani scope: serialization / matcher ABI only")
    return 0


if __name__ == "__main__":
    sys.exit(main())
