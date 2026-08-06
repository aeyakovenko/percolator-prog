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
//!
//! Guarantee boundary: this is a complete census only for the bounded test world, whose portfolio
//! count is checked against the market's materialized-portfolio counter. It does not replace a
//! production whole-state enumeration proof or the charter's resolved/recovery claim model.

use super::*;

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
