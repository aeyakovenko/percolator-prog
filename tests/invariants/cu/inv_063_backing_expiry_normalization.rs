//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_probe_post_expiry_trade_cannot_charge_backing_fee`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//!
//! Current result on PR135 base `1082c060`: this test is an intentionally red public-route TDD
//! counterexample. It must remain visible until the fixed engine/program pin makes it green.

use super::*;

#[test]
fn v16_probe_post_expiry_trade_cannot_charge_backing_fee() {
    const PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const OPEN_Q: i128 = 1_000 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 50 * POS_SCALE as i128;
    const WINNING_DOMAIN: usize = 1;
    const EXPIRY_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 5_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.update_backing_fee_policy_with_cu(WINNING_DOMAIN as u16, 5_000, 0);
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 100_000, EXPIRY_SLOT);

    let trader_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let trader = env.create_portfolio(&trader_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&trader_owner, trader, 52_501);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &counterparty_owner,
        counterparty,
        OPEN_Q,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(0, 1, WINNING_MARK);
    for portfolio in [counterparty, trader] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(env.portfolio_state(trader).pnl.get(), 5_000);
    let (_, before) = env.market_state();
    assert_eq!(before.current_slot, 1);
    assert_eq!(
        before.source_backing_buckets[WINNING_DOMAIN].expiry_slot,
        EXPIRY_SLOT
    );
    let trader_capital_before = env.portfolio_state(trader).capital.get();
    let provider_earnings_before =
        before.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let market_account_before = env.svm.get_account(&env.market).unwrap();
    let trader_account_before = env.svm.get_account(&trader).unwrap();
    let counterparty_account_before = env.svm.get_account(&counterparty).unwrap();

    env.svm.expire_blockhash();
    let retained_trade = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(trader_owner.pubkey(), true),
                    AccountMeta::new(counterparty_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(trader, false),
                    AccountMeta::new(counterparty, false),
                ],
                data: ProgInstruction::TradeNoCpi {
                    asset_index: 0,
                    size_q: INCREASE_Q,
                    exec_price: WINNING_MARK,
                    fee_bps: 0,
                }
                .encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &trader_owner, &counterparty_owner],
        env.svm.latest_blockhash(),
    );

    env.svm.warp_to_slot(EXPIRY_SLOT + 1);
    let trade = env.svm.send_transaction(retained_trade);

    let trader_after = env.portfolio_state(trader);
    let (_, after) = env.market_state();
    let provider_earnings_after =
        after.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let charged = provider_earnings_after - provider_earnings_before;
    let mut extracted = 0;
    if charged != 0 {
        let ledger = env.backing_domain_ledger_account();
        let provider_dest = env.token_account(env.admin.pubkey(), 0);
        env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(
            ledger,
            provider_dest,
            WINNING_DOMAIN as u16,
            charged,
        );
        extracted = env.token_amount(provider_dest);
    }
    assert!(
        trade.is_err(),
        "retained post-expiry trade charged {charged} backing-fee atoms, extracted {extracted} real SPL atoms, and reduced trader capital {} -> {} while authenticated slot {} exceeded expiry {} and engine slot stayed {}",
        trader_capital_before,
        trader_after.capital.get(),
        EXPIRY_SLOT + 1,
        EXPIRY_SLOT,
        after.current_slot,
    );
    assert_eq!(extracted, 0, "expired support may not earn a trade fee");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_account_before,
        "the rejected stale trade must roll back the market"
    );
    assert_eq!(
        env.svm.get_account(&trader).unwrap(),
        trader_account_before,
        "the rejected stale trade must roll back the trader"
    );
    assert_eq!(
        env.svm.get_account(&counterparty).unwrap(),
        counterparty_account_before,
        "the rejected stale trade must roll back the counterparty"
    );

    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &counterparty_owner,
        counterparty,
        -INCREASE_Q,
        WINNING_MARK,
        0,
    );
}
