//! INV-029 - Positive claim bounds never understate.
//!
//! Normative obligation: each source domain's market-level positive-claim bound must cover every
//! exact, bucketed, pending, unresolved, and recovery claim attributed to that domain. Public
//! transitions may replace or burn a contribution only by changing the owning portfolio's bound
//! by the same amount.
//!
//! Evidence in this file (F over public LiteSVM routes):
//! `v16_program_source_claim_bounds_equal_complete_portfolio_attribution` generates two winners in
//! the same source domain through real trades and authenticated mark cranks, partially burns both
//! claims with a less-favorable authenticated mark, closes both positions, and converts their
//! released PnL in either claimant order. Settled peak losses become positive recovery claims in
//! the opposite source domain; the route then adds independent backing and converts those claims
//! too. After each public transition, an independent complete portfolio census requires:
//!
//! ```text
//! source_credit[d].positive_claim_bound_num
//!     == sum(portfolio.source_domains[d].source_claim_bound_num)
//! source_claim_bound_total_num
//!     == sum(source_credit[d].positive_claim_bound_num)
//! ```
//!
//! Each conversion additionally proves exact account, domain, and aggregate bound deltas while SPL
//! custody remains unchanged. The shared stateful runner applies the same census after every
//! successful generated public action across single/batch and CPI/no-CPI routes.
//! `v16_program_favorable_funding_claim_bounds_are_exact_across_routes_and_sides` adds eight
//! zero-mark-move worlds across those four routes and both position orientations. Authenticated
//! target lag creates nonzero funding while effective price remains fixed, so the positive claim
//! can only come from funding. The independent census runs after every public transition; the
//! winner's sole source-domain bound must equal its exact positive funding PnL, conversion burns
//! that bound exactly without moving custody, both users recover the original aggregate principal,
//! and unrelated portfolios remain byte- and token-identical.
//! `v16_program_stale_positive_claim_blocks_snapshot_until_exactly_materialized` covers the
//! prospective-claim boundary with eight more public worlds. An authenticated favorable mark is
//! committed while both account K/F snapshots remain stale and no positive claim has yet been
//! booked. Resolution must preserve the independently reconstructed stale/stored-position blocker;
//! settling only the winner must materialize its exact source claim but still cannot snapshot or
//! pay it. Only after the losing cohort is also settled may its principal realize part of the
//! claim and the payout snapshot capture the exact remaining junior face. This proves the deployed
//! protocol's stale-uncertainty envelope is a fail-closed barrier, not an understated denominator
//! that can become withdrawable.
//! The pinned production profile has no approximate claim-bound buckets or rebucketing route.
//! Every complete-account census also requires each domain's exact claim tracker to equal its
//! positive bound, so any future non-exact production term fails this suite until range and
//! rebucketing coverage is supplied.
//! `v16_program_partial_receipt_exactly_replaces_its_prior_claim_bound` reuses the independent
//! underfunded terminal lifecycle. Its shared transition oracle observes a genuine partial receipt
//! and proves the terminal ledger adds exactly `terminal_positive_claim_face * BOUND_SCALE`,
//! removes at least `prior_bound_contribution_num`, and leaves the unreceipted pool equal to the
//! independently scanned bound of every remaining positive-PnL portfolio. The latter equality
//! permits a same-call source-bound refinement without permitting another claimant's bound to be
//! erased. Once a snapshot exists, total terminal claim mass may not increase.
//!
//! Guarantee boundary: this is a complete census only for the bounded test world, whose portfolio
//! count is checked against the market's materialized-portfolio counter. It does not replace a
//! production whole-state enumeration proof. Approximate-bucket range/rebucketing coverage is
//! inapplicable while the executable source lock keeps that optional mechanism out of production.

use super::*;
use crate::support::fuzz_model::{
    verify_favorable_funding_claim_bound_route_matrix,
    verify_stale_claim_snapshot_barrier_route_matrix,
};

#[test]
fn v16_program_stale_positive_claim_blocks_snapshot_until_exactly_materialized() {
    let worlds = verify_stale_claim_snapshot_barrier_route_matrix([0xf3; 32])
        .unwrap_or_else(|error| panic!("INV-029 stale-claim snapshot matrix: {error}"));
    assert_eq!(worlds, 8);
}

#[test]
fn v16_program_favorable_funding_claim_bounds_are_exact_across_routes_and_sides() {
    let worlds = verify_favorable_funding_claim_bound_route_matrix([0xf2; 32])
        .unwrap_or_else(|error| panic!("INV-029 favorable-funding matrix: {error}"));
    assert_eq!(worlds, 8);
}

#[test]
fn v16_program_partial_receipt_exactly_replaces_its_prior_claim_bound() {
    let evidence = verify_resolved_claim_quote_delta()
        .expect("public underfunded exact-receipt replacement lifecycle");
    assert!(evidence.partial_receipt_seeded);
    assert!(evidence.claim_payout_atoms > 0);
    assert!(evidence.receipt_replacement_count > 0);
    assert!(evidence.exact_receipt_num > 0);
    assert!(evidence.replaced_bound_num >= evidence.exact_receipt_num);
    assert_eq!(evidence.final_engine_vault, evidence.final_spl_vault);
}

#[test]
fn v16_program_claim_bound_boundary_partition_exhausts_public_lifecycle_grid() {
    let mut case = 0u8;
    for position_units in [1u8, 4] {
        for price_move in [5u8, 6, 19, 20] {
            for reverse_conversion_order in [false, true] {
                let mut seed = [0x29; 32];
                seed[0] = case;
                verify_positive_claim_bound_attribution_lifecycle(
                    seed,
                    position_units,
                    price_move,
                    reverse_conversion_order,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "claim-bound boundary cell failed: units={position_units}, \
                         move={price_move}, reverse={reverse_conversion_order}: {error}"
                    )
                });
                case = case.checked_add(1).expect("bounded case count");
            }
        }
    }
    assert_eq!(case, 16);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_claim_bounds_equal_complete_portfolio_attribution(
        seed in any::<[u8; 32]>(),
        position_units in 1u8..=4,
        price_move in 5u8..=20,
        reverse_conversion_order in any::<bool>(),
    ) {
        let result = verify_positive_claim_bound_attribution_lifecycle(
            seed,
            position_units,
            price_move,
            reverse_conversion_order,
        );
        prop_assert!(
            result.is_ok(),
            "public source-claim attribution lifecycle diverged: {}",
            result.unwrap_err()
        );
    }
}
