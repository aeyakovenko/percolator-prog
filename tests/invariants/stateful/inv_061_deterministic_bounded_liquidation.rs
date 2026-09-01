//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: liquidation and terminal settlement are deterministic, risk reducing,
//! OI coherent, and bounded. A publicly created ADL-scaled winner must retain a finite public exit
//! after resolution regardless of whether the winner or loser submits `CloseResolved` first.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_resolved_adl_close_order_matrix_preserves_funded_exits` opens an ordinary
//! matched position, moves an authenticated mark, and uses permissionless cranks to create a winner
//! whose stored basis exceeds effective long OI. It drives both owner-signed automatic-crank
//! landing orders until both users are terminal. Before resolution and after every accepted close,
//! an independent active-leg census recomputes each side's ADL-effective OI from raw basis and A
//! snapshots and must equal the deployed market counters; each accepted close must mutate and make
//! both OI lanes nonincreasing until they are exactly zero. Each user receives exactly its funded
//! value, SPL and internal custody reconcile to zero, token supply is conserved, and both portfolio
//! accounts close.
//! `v16_program_multi_asset_adl_liquidation_is_order_local_and_exit_complete` builds a larger
//! three-user topology with two equal-risk target legs. Public first-wave liquidations make each
//! target leg ADL-scaled, then an authenticated account fee makes the combined portfolio
//! liquidatable. Four opening transports, both persisted leg orders, and both market-accrual
//! orders must select exactly the first live leg, match an independent close-size/fee oracle,
//! mutate only that asset's OI and insurance domains, frame both counterparties and every SPL
//! account, and restore health below the CU ceiling. All three owners must then clear their
//! remaining effective or reset-obligation legs, withdraw the exact non-fee value, and close while
//! the market remains Live.
//!
//! The shared INV-035 matrix extends selection to three unequal-loss assets and all six persisted
//! leg orders. The shared INV-086 bridge then takes a public 70,000,000-quantity liquidation across
//! all four trade transports through its exact 2,723-atom close, resolution, a genuine underfunded
//! receipt, a value-moving top-up, and terminal custody.
//!
//! Guarantee boundary: these finite matrices cover two-user terminal landing orders, three-asset
//! unequal-loss selection, and liquidation-to-partial-receipt composition. Close size is not
//! caller-controlled on the sole public crank, and INV-059 source-locks that surface.
//! Larger account partitions, multi-asset repeated liquidation episodes, transfer, retirement, and
//! remaining maximum-shape cross-products remain in the audit ledger. INV-059 separately owns two
//! authenticated fee-bearing episodes across every opening transport and preserves owner reduction.

use super::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 8) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_061_resolved_adl_close_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_resolved_adl_close_order_matrix_preserves_funded_exits(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = verify_resolved_adl_close_orders(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ResolvedAdlCloseOrder::ALL.len());
        for (expected, discovery) in ResolvedAdlCloseOrder::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.order, expected);
            prop_assert!(
                discovery.satisfies_invariant(),
                "resolved-ADL close-order invariant failed: {discovery:?}"
            );
        }
    }
}

#[test]
fn v16_program_multi_asset_adl_liquidation_is_order_local_and_exit_complete() {
    let discoveries = verify_multi_asset_adl_liquidation_permutations([0x61; 32])
        .expect("INV-061 multi-asset ADL liquidation permutations");
    assert_eq!(
        discoveries.len(),
        DiscoveryTradeRoute::ALL.len()
            * EqualRiskAssetOrder::ALL.len()
            * EqualRiskAssetOrder::ALL.len(),
        "must cross every opening transport, persisted leg order, and accrual order"
    );
    for discovery in &discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "multi-asset ADL liquidation invariant failed: {discovery:?}"
        );
    }

    for route_worlds in
        discoveries.chunks_exact(EqualRiskAssetOrder::ALL.len() * EqualRiskAssetOrder::ALL.len())
    {
        let control = route_worlds[0];
        for candidate in route_worlds.iter().copied() {
            assert_eq!(
                (
                    candidate.pre_long_a,
                    candidate.pre_short_a,
                    candidate.pre_raw_basis_q,
                    candidate.pre_effective_q,
                    candidate.pre_oi_q,
                    candidate.pre_certified_liq_deficit,
                    candidate.expected_close_q,
                    candidate.liquidation_fee,
                ),
                (
                    control.pre_long_a,
                    control.pre_short_a,
                    control.pre_raw_basis_q,
                    control.pre_effective_q,
                    control.pre_oi_q,
                    control.pre_certified_liq_deficit,
                    control.expected_close_q,
                    control.liquidation_fee,
                ),
                "leg/accrual order changed equal-risk topology: control={control:?}, candidate={candidate:?}"
            );
            assert_eq!(
                (
                    candidate.expected_liquidation_fee,
                    candidate.participant_payout,
                    candidate.final_insurance,
                    candidate.final_c_tot,
                    candidate.final_vault,
                ),
                (
                    control.expected_liquidation_fee,
                    control.participant_payout,
                    control.final_insurance,
                    control.final_c_tot,
                    control.final_vault,
                ),
                "leg/accrual order changed terminal equal-risk economics: control={control:?}, candidate={candidate:?}"
            );
        }
    }

    let route_stride = EqualRiskAssetOrder::ALL.len() * EqualRiskAssetOrder::ALL.len();
    for permutation in 0..route_stride {
        let control = discoveries[permutation];
        for route_index in 1..DiscoveryTradeRoute::ALL.len() {
            let candidate = discoveries[route_index * route_stride + permutation];
            assert_eq!(
                (
                    candidate.leg_order,
                    candidate.accrual_order,
                    candidate.first_active_asset,
                    candidate.selected_asset,
                    candidate.pre_long_a,
                    candidate.pre_short_a,
                    candidate.pre_raw_basis_q,
                    candidate.pre_effective_q,
                    candidate.pre_oi_q,
                    candidate.expected_close_q,
                    candidate.observed_oi_reduce_q,
                    candidate.post_effective_q,
                ),
                (
                    control.leg_order,
                    control.accrual_order,
                    control.first_active_asset,
                    control.selected_asset,
                    control.pre_long_a,
                    control.pre_short_a,
                    control.pre_raw_basis_q,
                    control.pre_effective_q,
                    control.pre_oi_q,
                    control.expected_close_q,
                    control.observed_oi_reduce_q,
                    control.post_effective_q,
                ),
                "opening transport changed selected liquidation state: control={control:?}, candidate={candidate:?}"
            );
            assert_eq!(
                (
                    candidate.pre_certified_liq_deficit,
                    candidate.post_certified_liq_deficit,
                    candidate.liquidation_fee,
                    candidate.expected_liquidation_fee,
                    candidate.insurance_domain_budget_delta,
                    candidate.owner_exit_steps,
                    candidate.participant_payout,
                    candidate.final_insurance,
                    candidate.final_c_tot,
                    candidate.final_vault,
                    candidate.final_spl_vault,
                ),
                (
                    control.pre_certified_liq_deficit,
                    control.post_certified_liq_deficit,
                    control.liquidation_fee,
                    control.expected_liquidation_fee,
                    control.insurance_domain_budget_delta,
                    control.owner_exit_steps,
                    control.participant_payout,
                    control.final_insurance,
                    control.final_c_tot,
                    control.final_vault,
                    control.final_spl_vault,
                ),
                "opening transport changed equal-risk terminal economics: control={control:?}, candidate={candidate:?}"
            );
        }
    }
}
