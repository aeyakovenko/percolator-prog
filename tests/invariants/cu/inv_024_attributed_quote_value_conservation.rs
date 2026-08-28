//! INV-024 - attributed quote-value conservation.
//!
//! Normative obligation: every successful public route has an attributed quote
//! debit and credit, and rejected value-moving routes commit no token or market
//! accounting delta. These regressions exercise complete LiteSVM wrapper routes
//! and assert aggregate custody, capital, insurance, backing, PnL, and fee
//! conservation across realistic trade, crank, liquidation, funding, and
//! withdrawal sequences.

use super::*;

// security.md sweep — cross-margin (#22/#32): one portfolio holds positions on TWO assets.
// Probe aggregate conservation and per-asset OI balance under shared-capital cross-margin.
#[test]
fn v16_attack_cross_margin_two_asset_conservation() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("cfg auth mark asset1");
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let (_, g) = env.market_state();
    assert_eq!(
        g.c_tot, 4_000_000,
        "no capital created/destroyed across two-asset cross-margin"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(
        g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q,
        "asset0 OI balanced"
    );
    assert_eq!(
        g.assets[1].oi_eff_long_q, g.assets[1].oi_eff_short_q,
        "asset1 OI balanced"
    );
    assert!(
        g.assets[1].oi_eff_long_q > 0,
        "asset1 position actually opened"
    );
}

// security.md sweep — cross-margin settlement (#9/#33): same portfolio long on two assets;
// asset0 rises (gain), asset1 falls (loss). Net should wash. Probe value creation/destruction
// and senior conservation across cross-asset settlement.
#[test]
fn v16_attack_cross_margin_divergent_moves_conserve() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg_mark = |env: &mut V16CuEnv, ai: u16, _slot: u64, _mark: u64, ix: ProgInstruction| {
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .unwrap_or_else(|e| panic!("asset{} mark: {}", ai, e));
    };
    cfg_mark(
        &mut env,
        1,
        0,
        100,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
            authority_epoch: 0,
        },
    );
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // asset0 up -> la gains
    cfg_mark(
        &mut env,
        1,
        10,
        90,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 1,
            now_slot: 10,
            mark_e6: 90,
            authority_epoch: 0,
        },
    ); // asset1 down -> la loses
       // crank both assets for both portfolios, two passes to converge §6.1/§6.2 warmup.
    for slot in [10u64, 11] {
        for ai in [0u16, 1] {
            for p in [pa, pb] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[ai, 1 - ai]),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                );
            }
        }
    }
    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert_eq!(
        total_equity, 4_000_000,
        "total equity conserved across divergent cross-asset moves"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
}

// security.md sweep — account confusion (#44/#45): pass wrong-type accounts where a portfolio is
// expected (the market account, the vault, an uninitialized account). Owner/discriminator checks
// security.md sweep - PermissionlessCrank pre-parse realloc rollback (#44/#48): the handler grows
// program-owned portfolio storage before decoding the portfolio header. A raw undersized program-owned
// security.md sweep - duplicate writable account aliasing (#26/#44/#48): several public helpers take
// an arbitrary program-owned writable "portfolio" account. The market slab is also program-owned and
// security.md sweep — loss-of-funds / DoS (#22/#30): after maintenance fees accrue over a long
// idle period, the user must still be able to withdraw their remaining (post-fee) capital. A bug
// here = funds locked (LoF). Probe: deposit, accrue fees, sync, then withdraw everything left.
#[test]
fn v16_attack_fee_accrual_does_not_lock_user_funds() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.update_maintenance_fee_policy_with_cu(0);
    // long idle period, then settle the maintenance fee.
    env.svm.warp_to_slot(500);
    env.sync_maintenance_fee_with_cu(p, None, 500);
    let remaining = state::read_portfolio(&env.svm.get_account(&p).unwrap().data)
        .unwrap()
        .capital
        .get();
    assert!(
        remaining > 0 && remaining < 1_000_000,
        "fees took some but not all capital (got {})",
        remaining
    );
    // user withdraws ALL remaining capital — must succeed, funds not locked.
    let (dest, _) = env.withdraw_with_cu(&owner, p, remaining);
    let got = {
        let d = env.svm.get_account(&dest).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    };
    assert_eq!(
        got, remaining,
        "user recovered full post-fee capital (no LoF)"
    );
    let after = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
    assert_eq!(after.capital.get(), 0, "capital fully withdrawn");
    let (_, g) = env.market_state();
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation after fee+withdraw"
    );
}

// security.md sweep — insolvency / bad-debt socialization (#9/#33/#19): drive a small-capital
// SHORT underwater past its capital via a multi-slot up-move, settling each slot. The winner's
// profit must NOT be paid out of the vault past what's actually backed — senior conservation
// (vault >= c_tot + insurance) must hold and the winner's positive pnl must be capped by residual
// (the loser's bad debt is socialized via haircut, not printed).
#[test]
fn v16_attack_insolvency_bad_debt_is_socialized_not_printed() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250); // tiny capital -> will go insolvent
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    // Push the price up across slots (circuit breaker clamps ~100%/slot): 100 -> 200 -> 400.
    // Short's loss (size * (P-100)/POS_SCALE) exceeds its 250 capital -> bad debt.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        for acct in [long_account, short_account] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(acct, false),
                ],
                &[],
            );
        }
    }
    // Liquidate the insolvent short.
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );

    let lo = state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // Guard against a vacuous pass: confirm the scenario actually reached insolvency.
    assert!(
        g.assets[0].effective_price >= 300,
        "price actually moved up (got {})",
        g.assets[0].effective_price
    );
    assert_eq!(
        sh.capital.get(),
        0,
        "short was driven insolvent (capital wiped)"
    );
    // The crux: the vault never owes more (senior) than it holds, no matter the bad debt.
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation holds under insolvency"
    );
    let residual = g.vault.saturating_sub(g.c_tot).saturating_sub(g.insurance);
    let pos_pnl = lo.pnl.get().max(0) as u128 + sh.pnl.get().max(0) as u128;
    assert!(
        pos_pnl > residual,
        "setup must create more positive paper pnl than backed residual (residual {} pos_pnl {})",
        residual,
        pos_pnl
    );
    assert!(health_cert(&lo).valid, "winner cert refreshed");
    assert!(
        (health_cert(&lo).certified_equity as u128) <= lo.capital.get() + residual + 1,
        "winner certified equity bounded by capital+residual-backed support: eq={} cap={} residual={} paper_pnl={}",
        health_cert(&lo).certified_equity,
        lo.capital.get(),
        residual,
        lo.pnl.get()
    );
    // No capital was conjured: total realized capital <= total deposited.
    assert!(
        (lo.capital.get() + sh.capital.get()) <= 1_000_250,
        "no capital printed (got {})",
        lo.capital.get() + sh.capital.get()
    );
}

// security.md sweep — insurance backstop accounting (#33/#9): with a pre-funded insurance fund,
// bad debt from an insolvent loser should be absorbed by insurance so the winner is closer to
// whole, WITHOUT insurance going negative or the vault being over-credited. Probe: same insolvency
// as batch 16 but with seeded insurance; assert insurance never underflows and senior conservation
// holds (vault >= c_tot + insurance) with insurance accounted, no value printed.
#[test]
fn v16_attack_insurance_backstop_absorbs_bad_debt_no_underflow() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000); // junior backstop
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    let (_, g_before) = env.market_state();
    let ins_before = g_before.insurance;
    let vault_before = g_before.vault;

    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        for acct in [long_account, short_account] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(acct, false),
                ],
                &[],
            );
        }
    }
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );

    let lo = state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // insurance is a u128 accumulator: it must never wrap/underflow under bad-debt absorption.
    assert!(
        g.insurance <= ins_before,
        "insurance only spent (not conjured): {} <= {}",
        g.insurance,
        ins_before
    );
    // vault token balance is not increased by the bad-debt event (no minting).
    assert!(
        g.vault <= vault_before,
        "vault not over-credited: {} <= {}",
        g.vault,
        vault_before
    );
    // senior conservation with insurance fully accounted.
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation with insurance backstop"
    );
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= lo.pnl.get().max(0) + sh.pnl.get().max(0),
        "winner profit backed by residual"
    );
}

// security.md sweep — fee bounds / overflow (#37/#19): TradeNoCpi's fee_bps is caller-supplied. An
// out-of-range fee_bps must be rejected (bounded by max_trading_fee_bps), never overflow or drain
// beyond capital. A valid max fee must accrue to insurance with exact conservation.
#[test]
fn v16_attack_trade_fee_bps_bounded_and_conserving() {
    let mut env = V16CuEnv::new(); // default max_trading_fee_bps = 10_000
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    // out-of-range fee_bps must be rejected with no state change.
    for bad in [u64::MAX, 10_001u64, 50_000] {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, bad);
        assert!(
            r.is_err(),
            "fee_bps {} > max_trading_fee_bps must be rejected",
            bad
        );
    }
    let (_, g0) = env.market_state();
    assert_eq!(
        g0.assets[0].oi_eff_long_q, 0,
        "no OI from rejected over-fee trades"
    );
    assert_eq!(g0.c_tot, 2_000_000, "no capital moved by rejected trades");

    // valid max fee (10_000 bps = 100% of notional) succeeds and accrues to insurance.
    env.svm.expire_blockhash();
    let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 10_000);
    assert!(r.is_ok(), "max valid fee_bps should succeed: {:?}", r);
    let (_, g1) = env.market_state();
    // fee moved capital -> insurance internally; vault unchanged, conservation exact.
    assert_eq!(
        g1.vault, 2_000_000,
        "fee is internal: vault unchanged (no tokens created/destroyed)"
    );
    assert_eq!(
        g1.vault,
        g1.c_tot + g1.insurance,
        "exact conservation: vault == c_tot + insurance"
    );
    assert!(g1.insurance > 0, "fee actually accrued to insurance");
    // fee never exceeds the traded notional (bounded), so neither party is over-drained.
    let notional = 100u128; // POS_SCALE @ 100
    assert!(
        g1.insurance <= 2 * notional,
        "fee bounded by ~notional per side (insurance {})",
        g1.insurance
    );
    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    assert!(
        a.capital.get() > 0 && b.capital.get() > 0,
        "fee did not drain either party to zero"
    );
}

// security.md sweep — accounting drift under churn (#32/#35): interleaved deposits, withdrawals, and
// a trade open/close must never drift the aggregates. At every checkpoint c_tot == Σ(capitals) and
// vault == c_tot + insurance, and OI stays balanced. Catches any aggregate-update slippage.
#[test]
fn v16_attack_conservation_under_deposit_withdraw_trade_churn() {
    let mut env = V16CuEnv::new();
    let a = Keypair::new();
    let pa = env.create_portfolio(&a);
    let b = Keypair::new();
    let pb = env.create_portfolio(&b);
    let c = Keypair::new();
    let pc = env.create_portfolio(&c);
    let check = |env: &V16CuEnv, tag: &str| {
        let (_, g) = env.market_state();
        let sum: u128 = [pa, pb, pc]
            .iter()
            .map(|p| {
                state::read_portfolio(&env.svm.get_account(p).unwrap().data)
                    .unwrap()
                    .capital
                    .get()
            })
            .sum();
        assert_eq!(g.c_tot, sum, "[{}] c_tot == Σ capitals", tag);
        assert_eq!(
            g.vault,
            g.c_tot + g.insurance,
            "[{}] vault == c_tot + insurance",
            tag
        );
        assert_eq!(
            g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q,
            "[{}] OI balanced",
            tag
        );
    };
    env.deposit(&a, pa, 500_000);
    check(&env, "dep a");
    env.deposit(&b, pb, 800_000);
    check(&env, "dep b");
    env.svm.expire_blockhash();
    env.withdraw(&a, pa, 100_000);
    check(&env, "wd a");
    env.deposit(&c, pc, 300_000);
    check(&env, "dep c");
    // a (400k) trades vs b (800k): open then more churn.
    env.trade_asset_with_cu(0, &a, pa, &b, pb, POS_SCALE as i128, 100, 0);
    check(&env, "open trade");
    env.svm.expire_blockhash();
    env.deposit(&a, pa, 50_000);
    check(&env, "dep a2");
    env.svm.expire_blockhash();
    env.withdraw(&c, pc, 250_000);
    check(&env, "wd c");
    // close the trade (opposite).
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &a, pa, &b, pb, -(POS_SCALE as i128), 100, 0);
    check(&env, "close trade");
    // drain everyone fully.
    for (o, p) in [(&a, pa), (&b, pb), (&c, pc)] {
        let cap = state::read_portfolio(&env.svm.get_account(&p).unwrap().data)
            .unwrap()
            .capital
            .get();
        if cap > 0 {
            env.svm.expire_blockhash();
            env.withdraw(o, p, cap);
        }
    }
    check(&env, "drained");
    let (_, g) = env.market_state();
    assert_eq!(g.c_tot, 0, "all capital withdrawn");
    // total deposited (500k+800k+300k+50k = 1,650k) minus total withdrawn must net to insurance+vault residue.
    assert_eq!(
        g.vault, g.insurance,
        "vault fully accounted as insurance after full drain (no stranded value)"
    );
}

// security.md sweep — cross-margin liquidation fairness (#2/#22): a net-solvent cross-margined
// portfolio (gain on asset0 offsetting a loss on asset1) must NOT be liquidatable for value
// security.md sweep — convert+withdraw exactness (#33/#35): a winner converts backed +PnL to capital
// then withdraws through the real token vault. It must receive EXACTLY the backed amount — not more
// (value printing) nor less (LoF) — and the system fully drains with conservation intact.
#[test]
fn v16_attack_convert_then_withdraw_pays_exactly_backed_amount() {
    const BACKED: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, BACKED, 10);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.add_source_positive_pnl(p, 1, BACKED);
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let vault0 = env.market_state().1.vault;
    // convert the backed pnl into withdrawable capital.
    env.svm.expire_blockhash();
    let cr = env.send(
        env.convert_released_pnl_ix(p, BACKED),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    );
    assert!(cr.is_ok(), "convert backed pnl should succeed: {:?}", cr);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        BACKED,
        "capital == backed amount after convert"
    );
    // withdraw it all through the real vault.
    let (dest, _) = env.withdraw_with_cu(&owner, p, BACKED);
    let got = env.token_amount(dest) as u128;
    assert_eq!(
        got, BACKED,
        "winner receives EXACTLY the backed amount (no more, no less)"
    );
    let a = env.portfolio_state(p);
    assert_eq!(a.capital.get(), 0, "capital fully withdrawn");
    assert_eq!(a.pnl.get(), 0, "no residual pnl");
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault,
        vault0 - BACKED,
        "vault decreased by exactly the paid amount"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation after convert+withdraw"
    );
}

// security.md sweep — zero-amount input validation (#39): zero-amount operations must reject or be
// clean no-ops across deposit/withdraw/trade/topup — never corrupt state or conservation.
#[test]
fn v16_attack_zero_amount_inputs_are_safe() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();

    // deposit 0
    env.svm.expire_blockhash();
    let src = env.token_account(la.pubkey(), 0);
    let r_dep = env.send(
        env.deposit_ix(pa, 0),
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&la],
    );
    // withdraw 0
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, la.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let r_wd = env.send(
        env.withdraw_ix(pa, 0),
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&la],
    );
    // trade size 0
    env.svm.expire_blockhash();
    let r_tr = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, 0, 100, 0);

    // whatever the dispositions (reject or clean no-op), conservation must be intact and nothing moved.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged by zero-amount ops");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged by zero-amount ops");
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation intact");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI from zero-size trade");
    assert_eq!(
        env.token_amount(dest) as u128,
        0,
        "zero withdraw moved no tokens"
    );
    // capitals unchanged.
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        1_000_000,
        "pa capital unchanged"
    );
    assert_eq!(
        env.portfolio_state(pb).capital.get(),
        1_000_000,
        "pb capital unchanged"
    );
    let _ = (r_dep, r_wd, r_tr);
}

// security.md sweep — backing-bucket withdraw vs committed lien (#22/#48 LoF): a backing authority
// must NOT be able to withdraw principal that is currently LIENED to back a winner's positive PnL —
// doing so would strand the winner (loss of funds). The withdraw is gated by fresh_unliened_backing.
#[test]
fn v16_attack_backing_withdraw_cannot_strand_liened_winner() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000); // domain 1: 40 backing
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.add_source_positive_pnl(p, 1, 40); // liens the 40 to back p's +PnL
    let (_, g0) = env.market_state();
    let p0 = env.portfolio_state(p);
    assert!(
        p0.pnl.get() > 0,
        "winner has backed positive pnl (non-vacuous)"
    );
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    let market_id = env.asset_market_id(0);

    // try to withdraw the LIENED backing (full 40, and a partial 1) -> must reject.
    for amt in [40u128, 1] {
        env.svm.expire_blockhash();
        let r = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::WithdrawBackingBucket {
                domain: 1,
                market_id,
                authority_epoch: 0,
                amount: amt,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&env.admin],
        );
        assert!(
            r.is_err(),
            "withdrawing liened backing ({}) must reject (would strand winner)",
            amt
        );
        assert_eq!(
            env.token_amount(dest),
            0,
            "no tokens extracted from liened backing"
        );
    }
    // winner's backing intact: pnl still present and vault unchanged.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.vault, g0.vault,
        "vault unchanged by rejected backing withdraws"
    );
    assert_eq!(
        env.portfolio_state(p).pnl.get(),
        p0.pnl.get(),
        "winner's backed pnl preserved"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — deposit atomicity vs underfunded source (#35/#48): depositing more than the
// source token account holds must fail ATOMICALLY — capital must never be credited before the token
// transfer succeeds (a credit-before-transfer bug would let an attacker mint capital for free).
#[test]
fn v16_attack_deposit_underfunded_source_is_atomic() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    let (_, g0) = env.market_state();
    // source token account holds only 100, but we attempt to deposit 1_000_000.
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r = env.send(
        env.deposit_ix(p, 1_000_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "depositing more than the source holds must fail (token transfer cannot cover it)"
    );
    // ATOMIC: no capital credited, vault/c_tot unchanged, source untouched.
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        0,
        "no capital credited on failed deposit (no free mint)"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged by failed deposit");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged by failed deposit");
    assert_eq!(
        env.token_amount(source),
        100,
        "source token balance untouched"
    );
    // a valid deposit within balance still works afterward (state not corrupted).
    env.svm.expire_blockhash();
    let r2 = env.send(
        env.deposit_ix(p, 100),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r2.is_ok(),
        "valid in-balance deposit succeeds after the failed one"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        100,
        "valid deposit credits exactly 100"
    );
    assert_eq!(
        env.market_state().1.vault,
        g0.vault + 100,
        "vault grew by exactly the deposited 100"
    );
}

// security.md sweep — pnl_pos_tot aggregate integrity (#33, Bug-#10 neighborhood): pnl_pos_tot is the
// sum of positive account PnLs (the haircut denominator). It must stay EXACTLY equal to Σ max(0, pnl)
// as positions move through profit -> loss -> profit. A desync would mis-price the haircut.
#[test]
fn v16_attack_pnl_pos_tot_consistent_through_sign_flips() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );
    let crank_both = |env: &mut V16CuEnv, slot: u64| {
        for p in [sh, lo] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    };
    let check = |env: &V16CuEnv, tag: &str| {
        let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
        let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
        let (_, g) = env.market_state();
        let sum_pos = (a.pnl.get().max(0) + b.pnl.get().max(0)) as u128;
        assert_eq!(
            g.pnl_pos_tot,
            sum_pos,
            "[{}] pnl_pos_tot == Σ max(0,pnl) (a.pnl={} b.pnl={})",
            tag,
            a.pnl.get(),
            b.pnl.get()
        );
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "[{}] senior conservation",
            tag
        );
    };
    check(&env, "open");
    // price UP -> long wins; crank to settle.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 120);
    crank_both(&mut env, 10);
    env.svm.warp_to_slot(11);
    crank_both(&mut env, 11);
    check(&env, "long winning");
    // price DOWN below entry -> long now LOSES (pnl flips negative), short wins.
    env.svm.warp_to_slot(12);
    env.push_auth_mark_with_cu(12, 80);
    crank_both(&mut env, 12);
    env.svm.warp_to_slot(13);
    crank_both(&mut env, 13);
    check(&env, "long losing / short winning");
    // back UP to entry -> roughly flat.
    env.svm.warp_to_slot(14);
    env.push_auth_mark_with_cu(14, 100);
    crank_both(&mut env, 14);
    env.svm.warp_to_slot(15);
    crank_both(&mut env, 15);
    check(&env, "back to entry");
}

// security.md sweep — operation-sequence conservation (#32/#33 fuzz-lite): a long varied sequence of
// deposits/trades/flips/price-moves/cranks/withdrawals must never drift the core invariants. Checks
// real-vault==accounting, c_tot==Σcapitals, senior conservation, OI balance at every checkpoint.
#[test]
fn v16_attack_long_sequence_conservation() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let k: Vec<Keypair> = (0..4).map(|_| Keypair::new()).collect();
    let p: Vec<Pubkey> = k.iter().map(|kp| env.create_portfolio(kp)).collect();
    for i in 0..4 {
        env.deposit(&k[i], p[i], 1_000_000);
    }
    let check = |env: &V16CuEnv, tag: &str| {
        let (_, g) = env.market_state();
        let sum: u128 = p
            .iter()
            .map(|pp| {
                state::read_portfolio(&env.svm.get_account(pp).unwrap().data)
                    .unwrap()
                    .capital
                    .get()
            })
            .sum();
        assert_eq!(g.c_tot, sum, "[{}] c_tot == Σcapitals", tag);
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "[{}] senior conservation",
            tag
        );
        assert_eq!(
            g.vault as u64,
            env.token_amount(env.vault),
            "[{}] accounting vault == real vault balance",
            tag
        );
        assert_eq!(
            g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q,
            "[{}] OI balanced",
            tag
        );
    };
    check(&env, "deposits");
    // trades among the 4 accounts (open, partial close, flip).
    env.trade_asset_with_cu(
        0,
        &k[0],
        p[0],
        &k[1],
        p[1],
        (5_000 * POS_SCALE) as i128,
        100,
        0,
    );
    check(&env, "t1");
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &k[2],
        p[2],
        &k[3],
        p[3],
        (3_000 * POS_SCALE) as i128,
        100,
        0,
    );
    check(&env, "t2");
    // price move + cranks.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 108);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for pp in &p {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(*pp, false),
                ],
                &[],
            );
        }
    }
    check(&env, "after move+crank");
    // a deposit mid-stream + a flip.
    env.svm.expire_blockhash();
    env.deposit(&k[0], p[0], 200_000);
    check(&env, "mid-deposit");
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &k[1],
        p[1],
        &k[0],
        p[0],
        (2_000 * POS_SCALE) as i128,
        108,
        0,
    );
    check(&env, "flip");
    // price back, settle, close everyone out.
    env.svm.warp_to_slot(20);
    env.push_auth_mark_with_cu(20, 100);
    for slot in 20u64..=25 {
        env.svm.warp_to_slot(slot);
        for pp in &p {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(*pp, false),
                ],
                &[],
            );
        }
    }
    check(&env, "settled");
    // total real vault still equals accounting; no value created across the whole sequence.
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "final: accounting == real vault"
    );
    assert!(
        g.vault <= 4_200_000,
        "no value created (total deposited 4*1M + 200k)"
    );
}

// security.md sweep — cross-margin divergent close conservation (#33/#22): a portfolio long on asset0
// and short on asset1, both winning under divergent moves, closes both legs. Value must be conserved
// through the multi-asset settlement+close (no leakage, senior conservation, accounting==real vault).
#[test]
fn v16_attack_cross_margin_divergent_close_conserves() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .expect("mark cfg");
    };
    cfg(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
            authority_epoch: 0,
        },
    );
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    // la LONG asset0, SHORT asset1.
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, -(POS_SCALE as i128), 100, 0);
    // asset0 UP (la long wins), asset1 DOWN (la short wins) -> la wins both, lb loses both.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    cfg(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 1,
            now_slot: 10,
            mark_e6: 90,
            authority_epoch: 0,
        },
    );
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for ai in [0u16, 1] {
            for p in [pa, pb] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[ai, 1 - ai]),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                );
            }
        }
    }
    // close both legs at the moved prices.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 110, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 90, 0);
    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert_eq!(g.assets[0].oi_eff_long_q, 0, "asset0 flat after close");
    assert_eq!(g.assets[1].oi_eff_long_q, 0, "asset1 flat after close");
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert_eq!(
        total_equity, 4_000_000,
        "total equity conserved through divergent cross-asset close"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault balance"
    );
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
}

// security.md sweep — maintenance fee accrual on a positioned account (#32/#30): fees accrue
// INCREMENTALLY, bounded by max_accrual_dt per sync (anti-retroactivity: a cranker cannot charge a
// huge retroactive fee in one jump). Each increment must conserve (capital -> insurance), leave the
// position intact, and keep senior conservation + accounting==real-vault.
#[test]
fn v16_attack_maintenance_fee_with_open_position_conserves() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 5_000, 10_000, 1_000, 58,
    );
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.update_maintenance_fee_policy_with_cu(0);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    let cap0 = env.portfolio_state(pa).capital.get();
    let (_, g0) = env.market_state();

    // accrue fees incrementally across several slots (each crank+sync advances bounded by max_accrual_dt).
    let mut max_step: u128 = 0;
    let mut prev_cap = cap0;
    for slot in 1..=6u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pa, false),
            ],
            &[],
        );
        env.svm.expire_blockhash();
        env.sync_maintenance_fee_with_cu(pa, None, slot);
        let cap = env.portfolio_state(pa).capital.get();
        let step = prev_cap - cap;
        max_step = max_step.max(step);
        prev_cap = cap;
    }
    let cap_final = env.portfolio_state(pa).capital.get();
    let fee = cap0 - cap_final;
    let (_, g1) = env.market_state();
    assert!(
        fee > 0,
        "maintenance fee accrued on the positioned account (non-vacuous)"
    );
    // anti-retroactivity: no single step charges more than max_accrual_dt(1) * fee_per_slot(58) (with slack).
    assert!(
        max_step <= 58 * 3,
        "per-step fee bounded by the dt cap (no huge retroactive jump): max_step={}",
        max_step
    );
    // conservation: the fee moved capital -> insurance exactly.
    assert_eq!(
        g1.insurance,
        g0.insurance + fee,
        "fee moved capital -> insurance exactly"
    );
    assert_eq!(
        g1.c_tot,
        g0.c_tot - fee,
        "c_tot decreased by exactly the fee"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged (fee internal)");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault balance"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "fee accrual did not disturb the position"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI still balanced"
    );
    assert_domain_budget_remaining_total_consistent(&g1, "maintenance fee open position");
}

// security.md sweep — two-sided trade fee symmetry (#33/#37): a trade fee is charged to BOTH sides;
// each must pay exactly the same amount (no rounding asymmetry favoring one side), and the total fee
// must equal the insurance increase. Conservation: vault unchanged (fee is internal capital->insurance).
#[test]
fn v16_attack_two_sided_trade_fee_symmetric() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    let ca0 = env.portfolio_state(pa).capital.get();
    let cb0 = env.portfolio_state(pb).capital.get();
    // trade with a fee (notional 100 @ POS_SCALE, 100 bps).
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    let ca1 = env.portfolio_state(pa).capital.get();
    let cb1 = env.portfolio_state(pb).capital.get();
    let (_, g1) = env.market_state();
    let fee_a = ca0 - ca1;
    let fee_b = cb0 - cb1;
    assert!(fee_a > 0, "a fee was charged (non-vacuous)");
    assert_eq!(
        fee_a, fee_b,
        "both sides pay EXACTLY the same fee (no rounding asymmetry)"
    );
    // total fee -> insurance exactly; vault unchanged.
    assert_eq!(
        g1.insurance,
        g0.insurance + fee_a + fee_b,
        "total two-sided fee accrued to insurance"
    );
    assert_eq!(
        g1.c_tot,
        g0.c_tot - fee_a - fee_b,
        "c_tot decreased by exactly the total fee"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged (fee internal)");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — funding + maintenance fee combined (#32/#33): an account with a position accrues
// BOTH premium funding (zero-sum transfer to the counterparty) AND maintenance fees (to insurance).
// Both must apply together and conserve total value (no tokens created/destroyed; vault==deposited).
#[test]
fn v16_attack_funding_and_fee_combined_conserve() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        maintenance_fee_per_slot: 58,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    env.update_maintenance_fee_policy_with_cu(0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2); // premium
    for slot in 1..=6u64 {
        env.svm.warp_to_slot(slot);
        for p in [lo, sh] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
            env.svm.expire_blockhash();
            let market_before_sync = env.svm.get_account(&env.market).unwrap();
            let portfolio_before_sync = env.svm.get_account(&p).unwrap();
            if let Err(error) = env.try_sync_maintenance_fee_with_cu(p, None, slot) {
                assert!(
                    error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
                    "the optional post-crank fee sync must fail only at the live account lock: {error}"
                );
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap(),
                    market_before_sync,
                    "rejected post-crank fee sync must roll back market state"
                );
                assert_eq!(
                    env.svm.get_account(&p).unwrap(),
                    portfolio_before_sync,
                    "rejected post-crank fee sync must roll back portfolio state"
                );
            }
        }
    }
    let (_, g) = env.market_state();
    // funding actually accrued AND fees were charged (non-vacuous combination).
    assert!(
        g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0,
        "funding accrued"
    );
    assert!(g.insurance > 0, "maintenance fees accrued to insurance");
    // total value conserved: no tokens minted/burned, everything accounted within the vault.
    assert_eq!(
        g.vault,
        2 * DEPOSIT,
        "vault == total deposited (funding zero-sum + fees internal)"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under funding + fees"
    );
    assert_domain_budget_remaining_total_consistent(&g, "funding plus maintenance fee");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert!(
        total_equity + g.insurance as i128 <= g.vault as i128,
        "no value over-distributed"
    );
}

// security.md sweep — multi-party exposure transfer (#32/#33): A goes long vs B (short); then B closes
// by going long vs C (short). Exposure passes B->C. OI must stay balanced, B ends flat, and value is
// conserved through the chain (no leakage at the intermediary).
#[test]
fn v16_attack_exposure_transfer_chain_conserves() {
    let mut env = V16CuEnv::new();
    let a = Keypair::new();
    let pa = env.create_portfolio(&a);
    let b = Keypair::new();
    let pb = env.create_portfolio(&b);
    let c = Keypair::new();
    let pc = env.create_portfolio(&c);
    env.deposit(&a, pa, 1_000_000);
    env.deposit(&b, pb, 1_000_000);
    env.deposit(&c, pc, 1_000_000);
    let (_, g0) = env.market_state();
    // A long vs B short.
    env.trade_asset_with_cu(0, &a, pa, &b, pb, POS_SCALE as i128, 100, 0);
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        POS_SCALE as i128,
        "A long"
    );
    assert_eq!(
        env.portfolio_state(pb).legs[0].basis_pos_q.get(),
        -(POS_SCALE as i128),
        "B short"
    );
    // B closes by going long vs C (B: -1 -> 0; C: 0 -> -1). Exposure transferred B->C.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &b, pb, &c, pc, POS_SCALE as i128, 100, 0);
    assert_eq!(
        env.portfolio_state(pb).legs[0].basis_pos_q.get(),
        0,
        "B now flat (exposure passed to C)"
    );
    assert_eq!(
        env.portfolio_state(pc).legs[0].basis_pos_q.get(),
        -(POS_SCALE as i128),
        "C now short"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        POS_SCALE as i128,
        "A still long"
    );
    // OI balanced (A's long matched by C's short), conservation, accounting==real vault.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI balanced after transfer"
    );
    assert_eq!(
        g1.c_tot, g0.c_tot,
        "c_tot conserved (no fees) through the chain"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — undersized portfolio account handling (#44/#37): InitPortfolio on an account
// smaller than the required size must NOT cause an out-of-bounds write. The wrapper safely REALLOCS
// the account up to the required length (zero-initialized) before any portfolio write — then it works
// security.md sweep - legacy portfolio storage growth (#44/#48): after the matcher-config tail was
// added, an already-initialized portfolio with only the engine body must not become stuck. Deposit must
// grow the account through ensure_portfolio_storage_for_market_slots, preserve accounting, and leave the
// security.md sweep — max-leg multi-asset conservation (#32/#22): one portfolio holding positions on
// ALL asset slots must keep every invariant (c_tot==Σcapitals, accounting==real vault, per-asset OI
// balanced). Probes breadth across the full leg array.
#[test]
fn v16_attack_max_leg_multi_asset_conserves() {
    const N: u16 = 4;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(N, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    for ai in 1..N {
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: ai,
                now_slot: 0,
                initial_mark_e6: 100,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .expect("cfg mark");
    }
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    // open a long on every asset from pa vs pb.
    for ai in 0..N {
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(ai, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    }
    // every leg opened, conservation across all of them.
    let (_, g) = env.market_state();
    for ai in 0..N as usize {
        assert!(
            g.assets[ai].oi_eff_long_q > 0,
            "asset {} position opened",
            ai
        );
        assert_eq!(
            g.assets[ai].oi_eff_long_q, g.assets[ai].oi_eff_short_q,
            "asset {} OI balanced",
            ai
        );
    }
    let sum: u128 = [pa, pb]
        .iter()
        .map(|p| {
            state::read_portfolio(&env.svm.get_account(p).unwrap().data)
                .unwrap()
                .capital
                .get()
        })
        .sum();
    assert_eq!(g.c_tot, sum, "c_tot == Σcapitals across all legs");
    assert_eq!(
        g.c_tot, 10_000_000,
        "no value created across the multi-leg open"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    // crank every asset; still conserves.
    env.svm.warp_to_slot(5);
    for ai in 0..N {
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(ai),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pa, false),
            ],
            &[],
        );
    }
    let (_, g2) = env.market_state();
    assert_eq!(
        g2.vault as u64,
        env.token_amount(env.vault),
        "accounting==real vault after cranking all legs"
    );
    assert!(
        g2.vault >= g2.c_tot + g2.insurance,
        "senior conservation after crank"
    );
}

// security.md sweep — full fee'd round-trip conservation (#32/#33): deposit -> open (fee) -> close
// (fee) -> withdraw-all for both parties. Total tokens withdrawn + remaining insurance must equal the
// total deposited (every fee is accounted, nothing created or leaked).
#[test]
fn v16_attack_full_feed_roundtrip_conserves() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    // open with a fee, then close with a fee (both flat).
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 100, 100);
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(pa))),
        "la flat"
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(pb))),
        "lb flat"
    );
    // both withdraw all their capital.
    let cap_a = env.portfolio_state(pa).capital.get();
    let cap_b = env.portfolio_state(pb).capital.get();
    env.svm.expire_blockhash();
    let da = env.withdraw(&la, pa, cap_a);
    env.svm.expire_blockhash();
    let db = env.withdraw(&lb, pb, cap_b);
    let out = env.token_amount(da) as u128 + env.token_amount(db) as u128;
    let (_, g) = env.market_state();
    // total accounting closes: tokens out + remaining insurance == total deposited.
    assert_eq!(
        out + g.insurance,
        2_000_000,
        "out ({}) + insurance ({}) == deposited 2M",
        out,
        g.insurance
    );
    assert!(g.insurance > 0, "fees accrued to insurance (non-vacuous)");
    assert_eq!(g.c_tot, 0, "all capital withdrawn");
    assert_eq!(
        g.vault, g.insurance,
        "remaining vault is exactly the insurance (the fees)"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(out <= 2_000_000, "no value created: out <= deposited");
}

// security.md sweep — long sequence with a liquidation (#32/#33 fuzz-lite): a realistic flow including
// an insolvency+liquidation event must keep accounting==real-vault and senior conservation at every
// checkpoint, with no value created across the whole sequence.
#[test]
fn v16_attack_sequence_with_liquidation_conserves() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    let big = Keypair::new();
    let pbig = env.create_portfolio(&big);
    let thin = Keypair::new();
    let pthin = env.create_portfolio(&thin);
    let cp = Keypair::new();
    let pcp = env.create_portfolio(&cp);
    env.deposit(&big, pbig, 5_000_000);
    env.deposit(&thin, pthin, 250);
    env.deposit(&cp, pcp, 5_000_000);
    let check = |env: &V16CuEnv, tag: &str| {
        let (_, g) = env.market_state();
        assert_eq!(
            g.vault as u64,
            env.token_amount(env.vault),
            "[{}] accounting == real vault",
            tag
        );
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "[{}] senior conservation",
            tag
        );
        let sum: u128 = [pbig, pthin, pcp]
            .iter()
            .map(|p| {
                state::read_portfolio(&env.svm.get_account(p).unwrap().data)
                    .unwrap()
                    .capital
                    .get()
            })
            .sum();
        assert_eq!(g.c_tot, sum, "[{}] c_tot == Σcapitals", tag);
    };
    check(&env, "deposits");
    // big long vs thin short.
    env.trade_with_cu(&big, pbig, &thin, pthin, POS_SCALE as i128, 100, 0);
    check(&env, "open");
    // price up -> thin insolvent.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pthin, false),
            ],
            &[],
        );
    }
    check(&env, "thin insolvent");
    // Liquidate thin.
    env.crank(
        pthin,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    check(&env, "after liquidation");
    // crank big, then cp trades (fresh activity post-liquidation).
    env.svm.warp_to_slot(3);
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pbig, false),
        ],
        &[],
    );
    check(&env, "settled");
    let (_, g) = env.market_state();
    assert!(
        g.vault <= 10_000_250,
        "no value created across the whole sequence (deposited 5M+250+5M)"
    );
}

// security.md sweep — self-crank maintenance fee (#32): an account syncing its OWN maintenance fee as
// the cranker (cranker_portfolio == self) must still conserve — the fee splits into the cranker share
// (back to self) and insurance, totaling exactly the fee charged. No value created by self-cranking.
#[test]
fn v16_attack_self_crank_maintenance_fee_conserves() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 580,
    );
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    env.deposit(&la, pa, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(4_000); // cranker takes 40%
    let cap0 = env.portfolio_state(pa).capital.get();
    let (_, g0) = env.market_state();
    // la syncs its OWN fee, naming ITSELF as the cranker.
    env.svm.warp_to_slot(10);
    env.sync_maintenance_fee_with_cu(pa, Some(pa), 10);
    let cap1 = env.portfolio_state(pa).capital.get();
    let (_, g1) = env.market_state();
    // net effect on la's capital = -(fee) + (cranker share). insurance += (fee - cranker share).
    let net_loss = cap0.saturating_sub(cap1);
    let insurance_gain = g1.insurance - g0.insurance;
    // total value conserved: la's net loss == insurance gain (the cranker share returned to la nets out).
    assert_eq!(
        net_loss, insurance_gain,
        "self-crank: la's net loss == insurance gain (fee fully conserved)"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged (fee internal)");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert_eq!(g1.c_tot, cap1, "c_tot == la's capital (single account)");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    // la can't gain from self-cranking: its capital did not increase.
    assert!(
        cap1 <= cap0,
        "self-cranking never increases the caller's capital (no value extraction)"
    );
}

// security.md sweep — multi-account asymmetric premium funding zero-sum (#32/#33): funding redistributes
// value between longs and shorts per-leg via floored math. With UNEQUAL leg sizes across 3 accounts the
// per-account floors need not cancel exactly. Attacker goal: drive the rounding so the system MINTS net
// value (vault grows / senior conservation breaks) or pays longs more than shorts lose into spendable
// capital. Protection: funding never touches the vault and any rounding excess is unbacked junior pnl.
#[test]
fn v16_attack_multi_account_asymmetric_funding_conserves() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 50_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let l1o = Keypair::new();
    let l1 = env.create_portfolio(&l1o); // long 3S
    let l2o = Keypair::new();
    let l2 = env.create_portfolio(&l2o); // long 1S
    let sho = Keypair::new();
    let sh = env.create_portfolio(&sho); // short 4S (counterparty to both)
    for (o, p) in [(&l1o, l1), (&l2o, l2), (&sho, sh)] {
        env.deposit(o, p, DEPOSIT);
    }
    // Build matched OI with UNEQUAL longs: long1=3S and long2=1S, short absorbs 4S total.
    env.trade_with_cu(
        &l1o,
        l1,
        &sho,
        sh,
        (3 * POS_SCALE) as i128,
        INITIAL_PRICE,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_with_cu(&l2o, l2, &sho, sh, POS_SCALE as i128, INITIAL_PRICE, 0);

    // premium: push the mark above the index so funding accrues, then crank all three over several slots.
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    for slot in 1..=6u64 {
        env.svm.warp_to_slot(slot);
        for p in [l1, l2, sh] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    }
    let g = env.market_state().1;
    // non-vacuity: funding actually accrued.
    assert!(
        g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0,
        "funding accrued (non-vacuous)"
    );
    // CONSERVATION: funding is internal -> the vault is byte-exact 3*DEPOSIT, nothing minted/burned.
    // (Premium funding routes a protocol "premium cut" to insurance — internal redistribution, NOT
    // minting: the total vault is unchanged and insurance stays bounded by the conserved vault.)
    assert_eq!(
        g.vault,
        3 * DEPOSIT,
        "vault == total deposited (funding mints nothing despite unequal legs)"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(
        g.insurance <= g.vault,
        "insurance (incl. premium cut) bounded by the conserved vault"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under asymmetric funding"
    );
    // OI stayed balanced through all the funding cranks.
    assert_eq!(
        g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q,
        "OI balanced"
    );
    // senior side never exceeds what was deposited: sum(capital + realized losses) <= deposits.
    let mut senior: i128 = 0;
    for p in [l1, l2, sh] {
        let a = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
        senior += a.capital.get() as i128 + a.pnl.get().min(0);
    }
    assert!(
        senior <= (3 * DEPOSIT) as i128,
        "senior value (capital + realized losses) never exceeds deposits: {}",
        senior
    );
}

// security.md sweep — multi-asset cross-margin netting conservation (#9/#22 interaction): a portfolio
// holding a GAINING leg on asset 0 and a LOSING leg on asset 1 nets the two in equity (one offsets the
// other) while the requirement stays GROSS. Attacker goal: a simultaneous gain+loss across assets mints
// vault value or under-collateralizes the portfolio. Protection: price moves mint nothing (vault is
// byte-stable == deposits), the portfolio stays solvent, and senior conservation holds.
#[test]
fn v16_attack_cross_margin_netting_conserves() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let xo = Keypair::new();
    let x = env.create_portfolio(&xo);
    let y0o = Keypair::new();
    let y0 = env.create_portfolio(&y0o);
    let y1o = Keypair::new();
    let y1 = env.create_portfolio(&y1o);
    env.deposit(&xo, x, 10_000);
    env.deposit(&y0o, y0, 1_000_000);
    env.deposit(&y1o, y1, 1_000_000);
    let total_deposits = 10_000u128 + 1_000_000 + 1_000_000;
    env.trade_asset_with_cu(0, &xo, x, &y0o, y0, POS_SCALE as i128, 100, 0); // x LONG asset0
    env.trade_asset_with_cu(1, &xo, x, &y1o, y1, -(POS_SCALE as i128), 100, 0); // x SHORT asset1
    assert_eq!(
        env.market_state().1.vault,
        total_deposits,
        "vault == deposits at open"
    );

    // asset0 UP -> x's long GAINS; asset1 UP -> x's short LOSES (offsetting legs).
    let admin = env.admin.insecure_clone();
    env.svm.warp_to_slot(2);
    let _ = env.push_auth_mark_with_cu(2, 110);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 2,
            mark_e6: 110,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    )
    .expect("push the asset-1 authenticated mark");
    for ai in [0u16, 1] {
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[ai, 1 - ai]),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(x, false),
            ],
            &[],
        );
    }

    let xs = env.portfolio_state(x);
    let g = env.market_state().1;
    // both legs live, requirement gross (covers both); portfolio solvent (equity covers maintenance).
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&xs)),
        2,
        "x holds both legs"
    );
    assert!(health_cert(&xs).valid, "x cert valid");
    assert!(
        health_cert(&xs).certified_equity >= health_cert(&xs).certified_maintenance_req as i128,
        "x solvent (equity >= maint req)"
    );
    assert!(
        g.assets[0].effective_price > 100 && g.assets[1].effective_price > 100,
        "both marks moved (non-vacuous)"
    );
    // NO MINT: the simultaneous gain+loss across assets nets internally — vault is byte-stable == deposits.
    assert_eq!(
        g.vault, total_deposits,
        "cross-asset gain+loss mints no vault value"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation across cross-margin netting"
    );
}

// [from pr125]
// LoF/conservation sweep — liquidation cranker share at the 100% boundary (liquidation_cranker_fee_share_
// bps == 10_000). Unlike the maintenance-fee share (routed through insurance), the liquidation fee is
// charged to the LIQUIDATED account's collateral and split between the cranker and insurance. At 100%
// share the cranker takes the ENTIRE fee and insurance gets ZERO — the extreme where a regression could
// over-pay the cranker beyond the fee (mint) or leak to insurance. Proves: cranker_reward == total_fee,
// insurance delta == 0, the reward never exceeds the fee, and the fee mints no vault tokens (internal
// [from pr125]
// LoF/conservation sweep — maintenance cranker share at the 100% boundary (cranker_share_bps == 10_000).
// The policy validation rejects > 10_000 but ALLOWS exactly 10_000, so a market can route the ENTIRE
// maintenance fee to the cranker. The fee passes THROUGH insurance: sync credits the full charged fee to
// insurance, then `reward = charged * share / 10_000` is moved out to the cranker, with `retained =
// charged - reward` staying. At 100% share reward == charged, so the cranker takes the whole fee and
// insurance nets ZERO — the extreme where a regression could underflow insurance (reward > available) or
// lose/double the fee. Proves conservation holds at the boundary: payer pays exactly the fee, the cranker
// receives all of it, insurance is unchanged, and the domain-budget total stays consistent. The existing
// `_is_bounded` test only exercises a 40% share, never the 100% edge.
#[test]
fn v16_attack_sync_maintenance_full_cranker_share_conserves_no_insurance_underflow() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let payer_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(10_000); // 100% cranker share — the boundary

    let insurance_before = env.market_state().1.insurance;

    env.svm.warp_to_slot(10);
    env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);

    let (_, group) = env.market_state();
    let payer = env.portfolio_state(payer_portfolio);
    let cranker = env.portfolio_state(cranker_portfolio);

    // Same fee as the 40%-share test (the fee does not depend on the share): 580.
    let fee = 100_000_000 - payer.capital.get();
    assert_eq!(
        fee, 580,
        "maintenance fee charged is the same regardless of share"
    );
    // 100% share -> the cranker receives the ENTIRE fee...
    assert_eq!(
        cranker.capital.get(),
        580,
        "cranker receives the full fee at 100% share"
    );
    // ...and NOTHING is retained to insurance (and no underflow: insurance is unchanged, not negative).
    assert_eq!(
        group.insurance, insurance_before,
        "at 100% share nothing is retained to insurance (no underflow, no double-credit)"
    );
    // Conservation: the payer's debit equals exactly what the cranker received (insurance net zero).
    assert_eq!(
        cranker.capital.get() + (group.insurance - insurance_before),
        fee,
        "fee fully conserved: cranker reward + retained insurance == charged"
    );
    assert_domain_budget_remaining_total_consistent(&group, "100% maintenance cranker share");
}

// security.md sweep — CureAndCancelClose deposit accounting (#35/#48): the cure's optional_deposit
// must credit capital EXACTLY once matching the token transfer (no free-mint), and reject atomically
// if the source is underfunded. Finding E covered withdraw-after-cure; this covers the deposit leg.
#[test]
fn v16_attack_cure_deposit_exact_and_atomic() {
    let mut env = V16CuEnv::new();
    // account A: cure WITH a funded deposit -> capital credited exactly, source drained.
    let a_owner = Keypair::new();
    let a = env.create_portfolio(&a_owner);
    env.deposit(&a_owner, a, 100);
    env.seed_cancellable_close_progress(a);
    let src_a = env.token_account_for_mint(env.mint, a_owner.pubkey(), 50);
    let (_, g_pre) = env.market_state();
    env.cure_and_cancel_close_with_cu(&a_owner, a, src_a, 50);
    assert_eq!(
        env.portfolio_state(a).capital.get(),
        150,
        "cure deposit credits capital exactly (100 + 50)"
    );
    assert_eq!(
        env.token_amount(src_a),
        0,
        "source token account drained by exactly the deposit"
    );
    let (_, g_mid) = env.market_state();
    assert_eq!(
        g_mid.vault,
        g_pre.vault + 50,
        "vault grew by exactly the cure deposit"
    );
    assert_eq!(
        g_mid.vault,
        g_mid.c_tot + g_mid.insurance,
        "conservation after cure deposit"
    );

    // account B: cure with optional_deposit > source balance -> reject ATOMICALLY (no free-mint).
    let b_owner = Keypair::new();
    let b = env.create_portfolio(&b_owner);
    env.deposit(&b_owner, b, 100);
    env.seed_cancellable_close_progress(b);
    let src_b = env.token_account_for_mint(env.mint, b_owner.pubkey(), 50);
    let vault_before_failed_cure = env.market_state().1.vault; // after B's deposit
    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(b),
            position_epoch: env.portfolio_position_epoch(b),
            optional_deposit: 1_000,
        },
        vec![
            AccountMeta::new(b_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b, false),
            AccountMeta::new(src_b, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&b_owner],
    );
    assert!(
        r.is_err(),
        "cure deposit exceeding source balance must reject"
    );
    assert_eq!(
        env.portfolio_state(b).capital.get(),
        100,
        "no capital credited on failed cure (no free-mint)"
    );
    assert_eq!(
        env.token_amount(src_b),
        50,
        "source untouched on failed cure"
    );
    let (_, g_end) = env.market_state();
    assert_eq!(
        g_end.vault, vault_before_failed_cure,
        "vault unchanged by failed cure"
    );
    assert_eq!(
        g_end.vault,
        g_end.c_tot + g_end.insurance,
        "conservation intact"
    );
    let _ = g_mid;
}

// security.md sweep — cross-margin leg close releases its margin (#9/#22 interaction): opening a 2nd leg
// adds its full requirement (gross, #145); closing one leg must SYMMETRICALLY remove that leg's
// requirement (no stale margin lock, and no under-margin by dropping too much). Attacker/edge goal: after
// closing a leg the requirement stays inflated (DoS lock) or collapses below the remaining leg's risk.
#[test]
fn v16_attack_cross_margin_leg_close_releases_its_margin() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let xo = Keypair::new();
    let x = env.create_portfolio(&xo);
    let yo = Keypair::new();
    let y = env.create_portfolio(&yo);
    env.deposit(&xo, x, 1_000_000);
    env.deposit(&yo, y, 1_000_000);

    // open leg 0 (LONG asset 0) -> req1; then leg 1 (SHORT asset 1) -> req2 == 2*req1 (gross).
    env.trade_asset_with_cu(0, &xo, x, &yo, y, POS_SCALE as i128, 100, 0);
    let req1 = health_cert(&env.portfolio_state(x)).certified_initial_req;
    env.trade_asset_with_cu(1, &xo, x, &yo, y, -(POS_SCALE as i128), 100, 0);
    let req2 = health_cert(&env.portfolio_state(x)).certified_initial_req;
    assert_eq!(
        req2,
        2 * req1,
        "two legs charge the gross sum (precondition)"
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(x))),
        2,
        "x has 2 legs"
    );

    // CLOSE leg 0 (opposite trade on asset 0) -> x should be left with only the asset-1 leg.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &xo, x, &yo, y, -(POS_SCALE as i128), 100, 0);
    let xs = env.portfolio_state(x);
    let req_after = health_cert(&xs).certified_initial_req;
    let g = env.market_state().1;

    // SYMMETRIC RELEASE: closing leg 0 removed exactly its requirement -> back to a single leg's req.
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&xs)),
        1,
        "asset-0 leg fully closed, one leg left"
    );
    assert_eq!(g.assets[0].oi_eff_long_q, 0, "asset-0 OI fully unwound");
    assert!(g.assets[1].oi_eff_short_q > 0, "asset-1 leg still open");
    assert_eq!(
        req_after, req1,
        "requirement drops back to exactly one leg's req (no stale lock, no under-margin)"
    );
    // x is still healthy (equity covers the remaining single-leg requirement).
    assert!(
        health_cert(&xs).valid && health_cert(&xs).certified_equity >= 0,
        "x healthy with the remaining leg"
    );
    assert!(
        (health_cert(&xs).certified_equity as u128) >= req_after,
        "equity still covers the remaining requirement"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

#[test]
fn v16_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);

    let source = env.deposit(&owner, portfolio, 1_000);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), 1_000);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 1_000);
    assert_eq!(group.c_tot, 1_000);
    assert_eq!(account.capital.get(), 1_000);

    let dest = env.withdraw(&owner, portfolio, 400);
    assert_eq!(env.token_amount(dest), 400);
    assert_eq!(env.token_amount(env.vault), 600);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 600);
    assert_eq!(group.c_tot, 600);
    assert_eq!(account.capital.get(), 600);

    let insurance_source = env.top_up_insurance(250);
    assert_eq!(env.token_amount(insurance_source), 0);
    assert_eq!(env.token_amount(env.vault), 850);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 250);
    assert_eq!(group.vault, 850);

    let backing_source = env.top_up_backing_bucket(1, 300, 10);
    assert_eq!(env.token_amount(backing_source), 0);
    assert_eq!(env.token_amount(env.vault), 1_150);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 250);
    assert_eq!(group.vault, 1_150);
    assert_eq!(group.c_tot, 600);
    assert_eq!(
        group.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(group.source_backing_buckets[1].expiry_slot, 10);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        300 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        300 * BOUND_SCALE
    );

    env.enable_live_insurance_withdrawal();
    let (insurance_dest, _withdraw_insurance_cu) = env.withdraw_insurance_with_cu(100);
    assert_eq!(env.token_amount(insurance_dest), 100);
    assert_eq!(env.token_amount(env.vault), 1_050);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 150);
    assert_eq!(group.vault, 1_050);
    assert_eq!(group.c_tot, 600);
}

#[test]
fn v16_bpf_perps_positive_smoke_cross_margin_pnl_convert_close_and_withdraw() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 100;
    const DEPOSIT: u128 = 2_000_000;
    const EXPECTED_PNL: i128 = 5;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, DEPOSIT);

    let open_asset0_cu = env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    assert_cu_within("perps smoke open asset[0]", open_asset0_cu, TRADE_CU_LIMIT);
    let open_asset1_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        -(POS_SCALE as i128),
        INITIAL_PRICE,
        0,
    );
    assert_cu_within("perps smoke open asset[1]", open_asset1_cu, TRADE_CU_LIMIT);

    let cross_open = env.portfolio_state(cross_account);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&cross_open)),
        2
    );
    assert_eq!(
        active_leg_for_asset(&cross_open, 0).basis_pos_q,
        POS_SCALE as i128
    );
    assert_eq!(
        active_leg_for_asset(&cross_open, 1).basis_pos_q,
        -(POS_SCALE as i128)
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);

    let mut refresh_steps = 0usize;
    for (portfolio, asset_index, label) in [
        (
            counterparty_account,
            0,
            "counterparty asset[0] loss refresh",
        ),
        (cross_account, 0, "cross account asset[0] gain refresh"),
        (
            counterparty_account,
            1,
            "counterparty asset[1] loss refresh",
        ),
        (cross_account, 1, "cross account asset[1] gain refresh"),
    ] {
        if let Some(cu) = env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        ) {
            refresh_steps += 1;
            assert_cu_within(label, cu, CRANK_CU_LIMIT);
        }
    }
    assert!(
        refresh_steps >= 2,
        "both portfolios must make public progress"
    );

    let cross_after_refresh = env.portfolio_state(cross_account);
    let counterparty_after_refresh = env.portfolio_state(counterparty_account);
    assert_eq!(
        cross_after_refresh.pnl.get(),
        EXPECTED_PNL,
        "cross-margin account should realize +5 while carrying two active legs"
    );
    assert_eq!(counterparty_after_refresh.pnl.get(), 0);
    assert_eq!(
        counterparty_after_refresh.capital.get(),
        DEPOSIT - EXPECTED_PNL as u128
    );

    let close_asset0_cu = env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        -(POS_SCALE as i128),
        ASSET0_MARK,
        0,
    );
    assert_cu_within(
        "perps smoke close asset[0]",
        close_asset0_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let close_asset1_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        POS_SCALE as i128,
        ASSET1_MARK,
        0,
    );
    assert_cu_within(
        "perps smoke close asset[1]",
        close_asset1_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let cross_flat = env.portfolio_state(cross_account);
    let counterparty_flat = env.portfolio_state(counterparty_account);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &cross_flat
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &counterparty_flat
    )));
    assert_eq!(cross_flat.pnl.get(), EXPECTED_PNL);
    assert_eq!(cross_flat.capital.get(), DEPOSIT);
    assert_eq!(
        counterparty_flat.capital.get(),
        DEPOSIT - EXPECTED_PNL as u128
    );

    let convert_cu =
        env.convert_released_pnl_with_cu(&cross_owner, cross_account, EXPECTED_PNL as u128);
    assert_cu_within(
        "perps smoke convert released pnl",
        convert_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let cross_after_convert = env.portfolio_state(cross_account);
    assert_eq!(cross_after_convert.pnl.get(), 0);
    assert_eq!(
        cross_after_convert.capital.get(),
        DEPOSIT + EXPECTED_PNL as u128
    );

    let cross_dest = env.withdraw(
        &cross_owner,
        cross_account,
        cross_after_convert.capital.get(),
    );
    let counterparty_dest = env.withdraw(
        &counterparty_owner,
        counterparty_account,
        counterparty_flat.capital.get(),
    );
    assert_eq!(
        env.token_amount(cross_dest) as u128,
        DEPOSIT + EXPECTED_PNL as u128
    );
    assert_eq!(
        env.token_amount(counterparty_dest) as u128,
        DEPOSIT - EXPECTED_PNL as u128
    );
    assert_eq!(env.token_amount(env.vault), 0);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.insurance, 0);
}

// security.md sweep - resolved top-up custody (#33/#44/#48): ClaimResolvedPayoutTopup is
// regression (security.md sweep): MTM settlement under a price move (§6.1 loss->capital,
// §6.2 profit->pnl warmup). After full winner->loser->winner cranking, total equity is
// conserved, the winner's +PnL is backed by the loser's realized-loss residual, and senior
// conservation (vault >= c_tot + insurance) holds. (Investigating a narrow-invariant probe
// that fired here confirmed the warmup settlement is order-robust once fully cranked.)
#[test]
fn v16_regression_mark_to_market_settles_conservation_under_price_move() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let (_, _g0) = env.market_state();

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // mark up 10%
    env.crank(
        pa,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    env.crank(
        pb,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    env.crank(
        pa,
        ProgInstruction::PermissionlessCrank {
            now_slot: 11,
            observations: crank_observations(0),
        },
    );

    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g1) = env.market_state();
    // Widened (correct) invariant: senior conservation holds, total equity conserved, and after
    // full settlement the winner's gain is credited and backed by the loser's realized loss.
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation: vault >= c_tot + insurance"
    );
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert_eq!(
        total_equity, 2_000_000,
        "total equity (capital+pnl) conserved across both accounts"
    );
    let residual = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    let pos_pnl = a.pnl.get().max(0) + b.pnl.get().max(0);
    assert!(
        residual >= pos_pnl,
        "positive PnL must be backed by residual (no un-backed winner)"
    );
}

// regression (security.md sweep): profit realization round-trip — open, mark up, settle,
// then close both legs. Total equity conserved, flat, senior conservation, +pnl backed.
#[test]
fn v16_regression_profit_realization_roundtrip_conserves() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    env.crank(
        pa,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    env.crank(
        pb,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    // Close both legs at the new mark.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 110, 0);

    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert_eq!(g.assets[0].oi_eff_long_q, 0, "flat after close");
    assert_eq!(g.assets[0].oi_eff_short_q, 0, "flat after close");
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert_eq!(
        total_equity, 2_000_000,
        "total equity conserved through open->mark->close"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
}

// regression (security.md sweep): value extraction (#33/#35) — after a winner realizes profit and
// closes, withdraw each leg's full capital through the REAL token vault. Attacker success = total tokens
// out > total deposited (value printed) OR vault drops below c_tot+insurance (unbacked extraction).
#[test]
fn v16_regression_profit_withdraw_no_value_printed() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // winner = long (la)
    env.crank(
        pa,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    env.crank(
        pb,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 110, 0); // both flat

    // Each withdraws its full capital through the token vault.
    let cap_a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data)
        .unwrap()
        .capital
        .get();
    let cap_b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data)
        .unwrap()
        .capital
        .get();
    env.svm.expire_blockhash();
    let dest_a = env.withdraw(&la, pa, cap_a);
    env.svm.expire_blockhash();
    let dest_b = env.withdraw(&lb, pb, cap_b);

    let bal = |env: &V16CuEnv, k: &Pubkey| -> u64 {
        let d = env.svm.get_account(k).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap())
    };
    let out = bal(&env, &dest_a) as u128 + bal(&env, &dest_b) as u128;
    assert!(
        out <= 2_000_000,
        "no value printed: tokens out {} <= deposited 2_000_000",
        out
    );
    let (_, g) = env.market_state();
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation after profit withdraws"
    );
}

// security.md sweep — cross-margin insolvency (#9/#33/#22): a portfolio short on TWO assets is
// driven underwater on BOTH until its combined loss exceeds shared capital. Cross-asset bad debt
// must still be socialized, not printed: senior conservation holds and the winner is capped by
// residual. Probes the interaction of cross-margin shared capital with multi-asset insolvency.
#[test]
fn v16_regression_cross_margin_insolvency_no_value_extraction() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .expect("asset1 mark cfg");
    };
    cfg(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
            authority_epoch: 0,
        },
    );
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let cp_owner = Keypair::new();
    let cp = env.create_portfolio(&cp_owner);
    env.deposit(&victim_owner, victim, 250); // tiny shared capital
    env.deposit(&cp_owner, cp, 2_000_000);
    // victim SHORT on both assets (negative size on account_a); cp takes the long side.
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        -(POS_SCALE as i128),
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        -(POS_SCALE as i128),
        100,
        0,
    );

    // drive BOTH asset marks up over two slots: shorts lose, combined loss > 250 capital.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        cfg(
            &mut env,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                observation_sequence: slot + 1,
                asset_index: 1,
                now_slot: slot,
                mark_e6: mark,
                authority_epoch: 0,
            },
        );
        for ai in [0u16, 1] {
            for p in [victim, cp] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[ai, 1 - ai]),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                );
            }
        }
    }
    // Deep insolvency must remain permissionlessly reducible. Each successful call performs one
    // bounded engine-selected step; it may reduce exposure or escalate to terminal recovery, but
    // it cannot move custody or create value.
    let position_abs = |account: &PortfolioAccountV16| -> u128 {
        [0usize, 1]
            .into_iter()
            .map(|asset_index| {
                if has_active_leg_for_asset(account, asset_index) {
                    active_leg_for_asset(account, asset_index)
                        .basis_pos_q
                        .unsigned_abs()
                } else {
                    0
                }
            })
            .sum()
    };
    let victim_before_progress = env.portfolio_state(victim);
    let exposure_before = position_abs(&victim_before_progress);
    let (_, group_before_progress) = env.market_state();
    let vault_tokens_before = env.token_amount(env.vault);
    let mut successful_steps = 0usize;
    for _ in 0..8 {
        if env.market_state().1.mode != MarketModeV16::Live {
            break;
        }
        env.svm.expire_blockhash();
        let step = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: vec![
                    CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    },
                    CrankObservationHint {
                        asset_index: 1,
                        oracle_accounts: 0,
                    },
                ],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(victim, false),
            ],
            &[],
        );
        if let Ok(cu) = step {
            successful_steps += 1;
            assert_cu_within("cross-margin insolvency progress", cu, CRANK_CU_LIMIT);
        }
    }
    let victim_after_progress = env.portfolio_state(victim);
    let (_, group_after_progress) = env.market_state();
    assert!(successful_steps != 0);
    assert!(
        position_abs(&victim_after_progress) < exposure_before
            || group_after_progress.mode != MarketModeV16::Live,
        "bounded public cranks must reduce exposure or enter terminal recovery"
    );
    assert_eq!(group_after_progress.vault, group_before_progress.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    assert!(
        group_after_progress.vault >= group_after_progress.c_tot + group_after_progress.insurance
    );

    let v = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // non-vacuity: victim actually insolvent on a real up-move on both assets.
    assert!(
        g.assets[0].effective_price >= 300 && g.assets[1].effective_price >= 300,
        "both prices moved up"
    );
    assert_eq!(
        v.capital.get(),
        0,
        "victim's shared capital wiped by cross-asset losses"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under cross-margin insolvency"
    );

    // The winner has paper PnL against the insolvent counterparty, but it is uncollectable above
    // the backed residual. The real safety guarantee is that conversion/withdrawal cannot extract
    // more than the vault can support.
    let (_, g2) = env.market_state();
    assert_eq!(
        g2.vault, 2_000_250,
        "no tokens minted despite cross-margin insolvency"
    );
    assert!(
        g2.vault >= g2.c_tot + g2.insurance,
        "senior conservation persists under growing paper pnl"
    );

    // The winner's ConvertReleasedPnl is residual-bounded: it can NEVER pull paper pnl into capital
    // beyond the residual backing, no matter the pnl figure.
    let residual2 = (g2.vault - g2.c_tot - g2.insurance) as u128;
    assert!(
        env.portfolio_state(cp).pnl.get().max(0) as u128 > residual2,
        "setup must leave winner paper pnl above backed residual"
    );
    let cap_before = env.portfolio_state(cp).capital.get();
    let market_before_conversion = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_conversion = env.svm.get_account(&cp).unwrap();
    env.svm.expire_blockhash();
    let conversion = env.send(
        env.convert_released_pnl_ix(cp, 1_000_000_000),
        vec![
            AccountMeta::new(cp_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cp, false),
        ],
        &[&cp_owner],
    );
    let converted = env.portfolio_state(cp).capital.get() - cap_before;
    match conversion {
        Ok(_) => assert!(
            converted > 0,
            "an accepted conversion must move a positive backed amount"
        ),
        Err(error) => {
            assert!(
                error.contains("Custom(19)")
                    || error.contains("custom program error: 0x13")
                    || error.contains("Custom(21)")
                    || error.contains("custom program error: 0x15"),
                "an unavailable conversion must fail at stale-state or realizability admission: {error}"
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_conversion,
                "rejected conversion must roll back market accounting"
            );
            assert_eq!(
                env.svm.get_account(&cp).unwrap(),
                portfolio_before_conversion,
                "rejected conversion must roll back the winner portfolio"
            );
            assert_eq!(converted, 0, "rejected conversion cannot create capital");
        }
    }
    assert!(
        converted <= residual2,
        "winner conversion bounded by residual ({} <= {})",
        converted,
        residual2
    );

    // And total tokens the winner can actually pull out never exceed the vault.
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, cp_owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let cap_now = env.portfolio_state(cp).capital.get();
    assert!(
        cap_now > 0,
        "the withdrawal probe must target funded capital"
    );
    let market_before_withdraw = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_withdraw = env.svm.get_account(&cp).unwrap();
    let vault_before_withdraw = env.svm.get_account(&env.vault).unwrap();
    let dest_before_withdraw = env.svm.get_account(&dest).unwrap();
    let withdrawal = env.send(
        env.withdraw_ix(cp, cap_now),
        vec![
            AccountMeta::new(cp_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cp, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&cp_owner],
    );
    let out = {
        let d = env.svm.get_account(&dest).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    };
    match withdrawal {
        Ok(_) => assert!(out > 0, "an accepted withdrawal must move SPL value"),
        Err(error) => {
            assert!(
                error.contains("Custom(19)")
                    || error.contains("custom program error: 0x13")
                    || error.contains("Custom(21)")
                    || error.contains("custom program error: 0x15"),
                "an unavailable withdrawal must fail at stale-state or realizability admission: {error}"
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_withdraw,
                "rejected withdrawal must roll back market accounting"
            );
            assert_eq!(
                env.svm.get_account(&cp).unwrap(),
                portfolio_before_withdraw,
                "rejected withdrawal must roll back the winner portfolio"
            );
            assert_eq!(
                env.svm.get_account(&env.vault).unwrap(),
                vault_before_withdraw,
                "rejected withdrawal must roll back custody"
            );
            assert_eq!(
                env.svm.get_account(&dest).unwrap(),
                dest_before_withdraw,
                "rejected withdrawal must not credit the destination"
            );
            assert_eq!(out, 0, "rejected withdrawal cannot move SPL value");
        }
    }
    assert!(
        out <= 2_000_250,
        "winner cannot extract more tokens than the vault holds (got {})",
        out
    );
    let (_, g3) = env.market_state();
    assert!(
        g3.vault >= g3.c_tot + g3.insurance,
        "senior conservation after winner extraction attempt"
    );
}
