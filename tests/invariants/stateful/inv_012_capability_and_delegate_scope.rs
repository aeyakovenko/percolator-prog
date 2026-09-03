//! INV-012 - Capability and delegate scope.
//!
//! A retained CPI trade must bind the exact incarnation of the LP matcher
//! capability it intends to consume. Re-enabling the same program, context,
//! delegate, and fee cap is a new grant, not permission to revive transactions
//! retained under the old grant.
//!
//! This public LiteSVM matrix covers both CPI transports. Each world retains a
//! valid transaction, disables and re-enables the identical matcher tuple, and
//! requires the old transaction to reject with exact program-account, matcher,
//! token-supply, and lamport rollback. A transaction built after re-enable must
//! still execute, excluding an always-rejecting fix.

use crate::support::v16_svm::{
    MarketConfig, PublicTerminalClassification, PublicTerminalObservation, V16Svm,
};
use percolator::POS_SCALE;
use percolator_prog::error::PercolatorError;
use percolator_prog::ix::CrankObservationHint;

#[derive(Clone, Copy, Debug)]
enum CpiRoute {
    Single,
    Batch,
}

fn run_matcher_capability_aba_case(route: CpiRoute) {
    const TAKER: usize = 0;
    const LP: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut seed = [0x12; 32];
    seed[0] ^= match route {
        CpiRoute::Single => 1,
        CpiRoute::Batch => 2,
    };
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
        .expect("configure authenticated mark");

    let retained = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
    };
    let old_sequence = env.primary_portfolio_matcher_sequence(LP);
    env.set_matcher_config(LP, 0).expect("disable matcher");
    env.set_matcher_config(LP, 1)
        .expect("re-enable identical matcher tuple");
    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        old_sequence + 2,
        "the replacement grant must be a distinct matcher-config incarnation",
    );

    let market_before = env.market_data(false);
    let taker_before = env.primary_portfolio_data(TAKER);
    let lp_before = env.primary_portfolio_data(LP);
    let matcher_before = env.all_matcher_context_data();
    let supply_before = env.token_supply_observed();
    let taker_lamports_before = env.account_lamports(env.actors[TAKER].portfolio);
    let lp_lamports_before = env.account_lamports(env.actors[LP].portfolio);

    let stale_error = env
        .land_retained(retained)
        .expect_err("a transaction retained under the prior matcher grant must reject");
    let expected_error = format!("Custom({})", PercolatorError::EngineStale as u32);
    assert!(
        stale_error.contains(&expected_error),
        "stale matcher grant must fail with {expected_error}, got {stale_error}",
    );
    assert_eq!(env.market_data(false), market_before, "market rollback");
    assert_eq!(
        env.primary_portfolio_data(TAKER),
        taker_before,
        "taker rollback"
    );
    assert_eq!(env.primary_portfolio_data(LP), lp_before, "LP rollback");
    assert_eq!(
        env.all_matcher_context_data(),
        matcher_before,
        "matcher rollback"
    );
    assert_eq!(
        env.token_supply_observed(),
        supply_before,
        "SPL supply rollback"
    );
    assert_eq!(
        env.account_lamports(env.actors[TAKER].portfolio),
        taker_lamports_before,
        "taker lamport rollback",
    );
    assert_eq!(
        env.account_lamports(env.actors[LP].portfolio),
        lp_lamports_before,
        "LP lamport rollback",
    );

    let fresh = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
    };
    env.land_retained(fresh)
        .expect("the current matcher-config incarnation must remain live");
    let taker = env.primary_portfolio(TAKER);
    let lp = env.primary_portfolio(LP);
    assert!(
        taker.active_bitmap.iter().any(|word| word.get() != 0),
        "fresh taker transaction must install real exposure",
    );
    assert!(
        lp.active_bitmap.iter().any(|word| word.get() != 0),
        "fresh LP transaction must install real exposure",
    );
}

#[test]
fn v16_program_cpi_trades_bind_matcher_capability_incarnation() {
    for route in [CpiRoute::Single, CpiRoute::Batch] {
        run_matcher_capability_aba_case(route);
    }
}

#[test]
fn v16_program_position_mutation_invalidates_retained_matcher_enable() {
    const ATTACKER: usize = 0;
    const LP: usize = 1;
    const OPEN_PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 90;
    const SIZE_Q: i128 = 1_000 * POS_SCALE as i128;

    let config = MarketConfig {
        initial_price: OPEN_PRICE,
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new([0x92; 32], config);
    env.configure_auth_mark(false, 0, 1, OPEN_PRICE)
        .expect("configure authenticated mark");

    // The LP signs this while its matcher is synchronized. A later bilateral trade is supposed
    // to revoke that grant because it mutates the LP position without updating matcher state.
    let retained_enable = env.build_retained_matcher_config(LP, 1);
    let retained_sequence = env.primary_portfolio_matcher_sequence(LP);
    let retained_position_epoch = env.primary_portfolio_position_epoch(LP);

    env.begin_public_trace();
    env.trade_no_cpi(ATTACKER, LP, 0, SIZE_Q, OPEN_PRICE, 0)
        .expect("owners sign a bilateral open");
    env.trade_no_cpi(ATTACKER, LP, 0, -SIZE_Q, OPEN_PRICE, 0)
        .expect("owners sign a bilateral close");
    assert_eq!(env.primary_portfolio(ATTACKER).legs[0].basis_pos_q.get(), 0);
    assert_eq!(env.primary_portfolio(LP).legs[0].basis_pos_q.get(), 0);
    assert!(env.primary_portfolio_position_epoch(LP) > retained_position_epoch);
    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        retained_sequence,
        "position mutation must not consume the independent portfolio-request sequence",
    );
    let revoked =
        percolator_prog::state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP))
            .expect("decode revoked matcher grant");
    assert_eq!(revoked.enabled(), 0, "bilateral mutation revokes matcher");

    let market_before = env.market_data(false);
    let attacker_before = env.primary_portfolio_data(ATTACKER);
    let lp_before = env.primary_portfolio_data(LP);
    let matcher_before = env.all_matcher_context_data();
    let supply_before = env.token_supply_observed();
    let stale_result = env.land_retained(retained_enable);
    if let Err(error) = stale_result {
        let expected = format!("Custom({})", PercolatorError::EngineStale as u32);
        assert!(
            error.contains(&expected),
            "stale matcher enable must fail with {expected}, got {error}",
        );
        assert_eq!(env.market_data(false), market_before, "market rollback");
        assert_eq!(
            env.primary_portfolio_data(ATTACKER),
            attacker_before,
            "attacker rollback",
        );
        assert_eq!(env.primary_portfolio_data(LP), lp_before, "LP rollback");
        assert_eq!(
            env.all_matcher_context_data(),
            matcher_before,
            "matcher rollback"
        );
        assert_eq!(env.token_supply_observed(), supply_before, "SPL rollback");

        env.set_matcher_config(LP, 1)
            .expect("fresh LP authorization remains live");
        let current =
            percolator_prog::state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP))
                .expect("decode fresh matcher grant");
        assert_eq!(current.enabled(), 1);
        env.finish_public_trace()
            .validate_public_execution()
            .expect("fixed trace uses only public transactions with exact rollback");
        return;
    }

    // Pre-fix exploit: SetMatcherConfig copied the *current* position epoch into a request signed
    // under the old epoch, reviving a capability that automatic invalidation had revoked.
    let revived =
        percolator_prog::state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP))
            .expect("decode replayed matcher grant");
    assert_eq!(
        revived.enabled(),
        1,
        "stale request revived matcher authority"
    );
    let attacker_capital_before = env.primary_portfolio(ATTACKER).capital.get();
    let victim_capital_before = env.primary_portfolio(LP).capital.get();

    env.trade_cpi(ATTACKER, LP, 0, -SIZE_Q, 0, 0)
        .expect("attacker opens an LP position through the revived grant");
    assert_eq!(
        env.primary_portfolio(LP).legs[0].basis_pos_q.get(),
        SIZE_Q,
        "replayed grant creates fresh victim exposure without a fresh LP signature",
    );

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, ADVERSE_PRICE)
        .expect("honest oracle moves against the victim position");
    let observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: 0,
    }];
    env.crank(ATTACKER, 2, observation.clone())
        .expect("settle attacker");
    env.crank(LP, 2, observation).expect("settle victim");
    env.trade_no_cpi(ATTACKER, LP, 0, SIZE_Q, ADVERSE_PRICE, 0)
        .expect("victim takes the bounded owner-signed exit");

    for actor in [ATTACKER, LP] {
        let released = env.primary_portfolio(actor).pnl.get().max(0) as u128;
        if released != 0 {
            env.convert_released_pnl(actor, released)
                .expect("convert released terminal PnL");
        }
    }
    let attacker_payout = env.primary_portfolio(ATTACKER).capital.get();
    let victim_payout = env.primary_portfolio(LP).capital.get();
    let attacker_gain = attacker_payout
        .checked_sub(attacker_capital_before)
        .expect("adverse move must benefit attacker");
    let victim_loss = victim_capital_before
        .checked_sub(victim_payout)
        .expect("adverse move must debit victim");
    assert!(
        victim_loss > 0,
        "exploit must cause non-vacuous victim loss"
    );
    assert_eq!(
        attacker_gain, victim_loss,
        "attacker receives the victim loss"
    );

    let attacker_destination_before = env.token_amount(env.actors[ATTACKER].destination_token);
    let victim_destination_before = env.token_amount(env.actors[LP].destination_token);
    env.withdraw_primary(ATTACKER, attacker_payout)
        .expect("attacker withdraws exploit proceeds");
    env.withdraw_primary(LP, victim_payout)
        .expect("victim withdraws reduced capital");
    assert_eq!(
        u128::from(
            env.token_amount(env.actors[ATTACKER].destination_token) - attacker_destination_before,
        ),
        attacker_payout,
    );
    assert_eq!(
        u128::from(env.token_amount(env.actors[LP].destination_token) - victim_destination_before),
        victim_payout,
    );

    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("exploit trace uses public transactions and real SPL custody");
    let classification = trace
        .classify_terminal(PublicTerminalObservation {
            victim_loss_atoms: victim_loss,
            unauthorized_gain_atoms: attacker_gain,
            funded_value_remaining: 0,
            unresolved_obligation: 0,
            bounded_exit_succeeded: true,
            terminal_receipt_created: false,
            authorized_forfeit: false,
            required_exit_routes: 0,
            attempted_exit_routes: 0,
            progressing_exit_routes: 0,
        })
        .expect("classify public terminal loss");
    assert_eq!(
        classification,
        PublicTerminalClassification::LossOfFunds {
            victim_loss_atoms: victim_loss,
            unauthorized_gain_atoms: attacker_gain,
        },
    );
    panic!(
        "stale matcher-enable replay caused public LoF: victim_loss={victim_loss}, attacker_gain={attacker_gain}"
    );
}
