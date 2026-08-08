//! INV-004 - Position episode binding.
//!
//! Normative obligation: Reduction, close, conversion, claim, and forfeit consent applies only to
//! the exact economic position or recovery episode that existed when the owner signed it.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_position_episode_matrix_rejects_stale_consent_atomically` creates each old
//! episode, retains owner-signed consent, completes the episode through public transitions, then
//! creates a fresh episode at the same market/asset/portfolio IDs. The stale request must reject
//! with exact market, portfolio, vault, and SPL-supply rollback while preserving the replacement
//! exposure. A request built against the current episode must still land and change that exposure,
//! proving the rejection is neither vacuous nor a blanket risk-reduction DoS. No program-owned
//! bytes are injected. `v16_program_all_trade_routes_advance_position_episode_once_and_errors_do_not`
//! also proves that the episode is packed into the matcher-control word: matcher revoke/re-enable
//! preserves it, the separate matcher-sequence tail, and the deployed portfolio account length.
//!
//! Guarantee boundary: this certifies the two retained position-consent routes represented here;
//! other retained instruction families require their own identity and episode bindings.

use super::*;

#[test]
fn v16_program_all_trade_routes_advance_position_episode_once_and_errors_do_not() {
    use crate::support::v16_svm::{MarketConfig, V16Svm};
    use percolator_prog::constants;
    use percolator_prog::ix::{BatchTradeCpiLeg, BatchTradeLeg};

    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    let size_q = percolator::POS_SCALE as i128;

    for route in 0u8..4 {
        let mut seed = [0x04u8; 32];
        seed[0] = route;
        let mut env = V16Svm::new(
            seed,
            MarketConfig {
                initial_price: PRICE,
                actor_deposits: [DEPOSIT, DEPOSIT, 0, 0, 0],
                actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
                ..MarketConfig::default()
            },
        );
        env.configure_auth_mark(false, 0, 1, PRICE)
            .expect("configure route mark");
        let portfolio_ids = [env.primary_portfolio_id(0), env.primary_portfolio_id(1)];
        let portfolio_lens = [
            env.primary_portfolio_data(0).len(),
            env.primary_portfolio_data(1).len(),
        ];
        assert_eq!(
            portfolio_lens,
            [
                constants::PORTFOLIO_ACCOUNT_LEN,
                constants::PORTFOLIO_ACCOUNT_LEN,
            ],
            "position and matcher episodes must use the deployed fixed account layout"
        );

        let execute = |env: &mut V16Svm, signed_size: i128| {
            let market_id = env.primary_market_state().1.assets[0].market_id;
            match route {
                0 => env.trade_no_cpi(0, 1, 0, signed_size, PRICE, 0),
                1 => env.trade_cpi(0, 1, 0, signed_size, 0, 0),
                2 => env.batch_trade_no_cpi(
                    0,
                    1,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id,
                        size_q: signed_size,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                3 => env.batch_trade_cpi(
                    0,
                    1,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id,
                        size_q: signed_size,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                _ => unreachable!(),
            }
        };

        let before_open = [
            env.primary_portfolio_position_epoch(0),
            env.primary_portfolio_position_epoch(1),
        ];
        execute(&mut env, size_q).unwrap_or_else(|error| {
            panic!("route {route} must open through its public interface: {error}")
        });
        let after_open = [
            env.primary_portfolio_position_epoch(0),
            env.primary_portfolio_position_epoch(1),
        ];
        assert_eq!(after_open, [before_open[0] + 1, before_open[1] + 1]);

        env.set_matcher_config(1, 0)
            .expect("LP owner can revoke matcher after an episode change");
        assert_eq!(
            env.primary_portfolio_position_epoch(1),
            after_open[1],
            "matcher revocation reset the position episode"
        );
        env.set_matcher_config(1, 1)
            .expect("LP owner can restore matcher after an episode change");
        assert_eq!(
            env.primary_portfolio_position_epoch(1),
            after_open[1],
            "matcher re-enable reset the position episode"
        );
        assert_eq!(
            [
                env.primary_portfolio_data(0).len(),
                env.primary_portfolio_data(1).len(),
            ],
            portfolio_lens,
            "matcher or episode state reallocated a deployed portfolio"
        );

        assert!(
            execute(&mut env, 0).is_err(),
            "route {route} accepts zero size"
        );
        assert_eq!(
            [
                env.primary_portfolio_position_epoch(0),
                env.primary_portfolio_position_epoch(1),
            ],
            after_open,
            "route {route} advanced an episode on a rejected instruction"
        );

        execute(&mut env, -size_q).unwrap_or_else(|error| {
            panic!("route {route} must close through its public interface: {error}")
        });
        assert_eq!(
            [
                env.primary_portfolio_position_epoch(0),
                env.primary_portfolio_position_epoch(1),
            ],
            [after_open[0] + 1, after_open[1] + 1],
            "route {route} did not advance each changed portfolio exactly once"
        );
        assert_eq!(
            [env.primary_portfolio_id(0), env.primary_portfolio_id(1)],
            portfolio_ids,
            "position episodes must not replace portfolio incarnations"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_004_position_episode_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_position_episode_matrix_rejects_stale_consent_atomically(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_position_episode_replays(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), PositionEpisodeKind::ALL.len());
        for (kind, discovery) in PositionEpisodeKind::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.kind, kind);
            prop_assert!(
                discovery.satisfies_invariant(),
                "position-episode binding or nonvacuous current route failed: {:?}",
                discovery
            );
        }
    }
}
