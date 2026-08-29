//! INV-064 - insurance-withdrawal policy equivalence.
//!
//! The wrapper exposes one asset-scoped insurance withdrawal route in both live and resolved
//! modes. The same authority, budget, custody, and rollback rules must apply in either mode.
//! `v16_bpf_resolved_terminal_insurance_drains_dynamic_domain_after_positions_close` additionally
//! owns INV-027's insurance-withdrawal seniority row: loss-stale live withdrawal preserves market,
//! vault, and both funded portfolios byte-for-byte; both users then recover exact principal before
//! the authority receives only the exact residual insurance in terminal mode.

use super::*;

// The asset-scoped route is intentionally usable under the healthy-live withdrawal policy, but after
// resolution it must reject while c_tot != 0 (open capital still backed). Attacker goal: drain
// insurance out from under accounts that still hold capital. We pre-fund domain-0's budget so the
// available-amount gate passes in every world, isolating the resolved wind-down gate.
#[test]
fn v16_attack_resolved_asset_insurance_withdraw_requires_full_wind_down() {
    let mut env = V16CuEnv::new();
    // Fund domain-0 insurance budget so available_insurance(admin) >= the amount we attempt.
    env.top_up_insurance_domain_with_authority(&env.admin.insecure_clone(), 0, 1_000_000);
    let amount: u128 = 400_000;

    let attempt = |env: &mut V16CuEnv| {
        let dest = env.token_account(env.admin.pubkey(), 0);
        let withdraw = env.withdraw_insurance_asset_instruction(env.admin.pubkey(), 0, amount);
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            withdraw,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&env.admin.insecure_clone()],
        )
    };

    // Live mode is a discriminating control: the healthy-live policy admits this exact withdrawal.
    assert_eq!(
        env.market_state().1.mode,
        percolator::MarketModeV16::Live,
        "starts Live"
    );
    assert!(attempt(&mut env).is_ok(), "healthy live withdrawal works");

    // Open capital, then resolve. c_tot stays > 0 (depositor's capital is still on the book).
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 600_000);
    env.resolve();
    let g = env.market_state().1;
    assert_eq!(g.mode, percolator::MarketModeV16::Resolved, "resolved");
    assert!(
        g.c_tot > 0,
        "capital still open after resolve (non-vacuous gate)"
    );

    // Resolved but c_tot != 0 must reject: insurance still backs open capital.
    assert!(
        attempt(&mut env).is_err(),
        "WithdrawInsuranceAsset must reject while c_tot != 0 (capital still backed)"
    );

    // A fresh, identically funded, fully wound-down market admits the same amount, proving the
    // rejection above is the wind-down gate rather than authority, custody, or capacity.
    let mut env2 = V16CuEnv::new();
    env2.top_up_insurance_domain_with_authority(&env2.admin.insecure_clone(), 0, 1_000_000);
    env2.resolve();
    let g2 = env2.market_state().1;
    assert_eq!(
        g2.mode,
        percolator::MarketModeV16::Resolved,
        "control resolved"
    );
    assert_eq!(g2.c_tot, 0, "control fully wound down");
    assert!(
        attempt(&mut env2).is_ok(),
        "WithdrawInsuranceAsset succeeds once fully wound down (discriminating control)"
    );

    let g = env2.market_state().1;
    assert_eq!(
        g.vault as u64,
        env2.token_amount(env2.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — resolved-mode backing withdrawal wind-down gate (SOL-021/022): LP backing is the
// loss-absorption layer behind users. In RESOLVED mode handle_withdraw_backing_bucket (v16_program 7834)
// permits a withdrawal ONLY once materialized_portfolio_count == 0 AND c_tot == 0 — i.e. every user has
// been paid out and closed. If the backing_bucket_authority (or marketauth) could pull backing while
// resolved users still hold capital/claims, the vault would drop below what those users are owed (LOF).
// This is the backing parallel of v16_attack_withdraw_insurance_requires_full_wind_down (insurance), but a
// DISTINCT code path with a distinct condition (count+c_tot vs insurance wind-down). It was untested:
// 15135 covers the LIVE liened-winner case, not resolved-mode open capital. Non-vacuous: the same
// withdrawal succeeds on the live empty market first.
#[test]
fn v16_attack_resolved_backing_withdraw_requires_full_user_wind_down() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 1_000, 100_000); // domain 1 (asset-0 short) backing, admin-authorized
    let dest = env.token_account(env.admin.pubkey(), 0);

    // Sanity: on a live, user-free market the backing authority CAN withdraw — proves authority + path
    // are fine, so the resolved-mode rejection below is caused by the wind-down gate, not a precondition.
    env.svm.expire_blockhash();
    env.withdraw_backing_bucket_to_admin_token_with_cu(dest, 1, 100);

    // Open user capital, then resolve. c_tot + materialized_portfolio_count stay > 0.
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 600_000);
    env.resolve();
    let g = env.market_state().1;
    assert_eq!(g.mode, percolator::MarketModeV16::Resolved, "resolved");
    assert!(
        g.c_tot > 0,
        "user capital still open after resolve (non-vacuous gate)"
    );
    assert!(
        g.materialized_portfolio_count > 0,
        "user portfolio still materialized after resolve"
    );

    let vault_before = g.vault;
    let dest_before = env.token_amount(dest);
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id: g.assets[0].market_id,
            authority_epoch: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin.insecure_clone()],
    );
    assert!(
        r.is_err(),
        "resolved-mode backing withdrawal must reject while users still hold capital/claims"
    );
    let g_after = env.market_state().1;
    assert_eq!(
        g_after.vault, vault_before,
        "rejected resolved backing withdrawal must leave the vault untouched"
    );
    assert_eq!(
        env.token_amount(dest),
        dest_before,
        "no backing tokens may leave to the authority before users are wound down"
    );
    assert!(
        g_after.vault >= g_after.c_tot + g_after.insurance,
        "senior conservation intact"
    );
}

// security.md sweep — per-asset WithdrawInsuranceAsset budget conservation (#6): the asset insurance
// operator may withdraw accrued asset budget in LIVE mode, but NEVER more than the asset's remaining
// long+short budget, and partial withdrawals must debit the remaining budget so it cannot be double-drained.
// Attacker goal: withdraw an asset's insurance twice (or over its budget) to extract more than accrued.
#[test]
fn v16_attack_withdraw_insurance_domain_budget_cannot_be_overdrawn() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    // Credit domain 0's budget with 1_000_000 (Live mode). insurance and domain-0 budget both = 1M.
    env.top_up_insurance_domain_with_authority(&admin, 0, 1_000_000);
    let g = env.market_state().1;
    assert_eq!(g.mode, percolator::MarketModeV16::Live, "Live");
    assert_eq!(
        g.insurance_domain_budget[0], 1_000_000,
        "domain-0 budget funded"
    );

    let conserve = |env: &V16CuEnv| {
        let g = env.market_state().1;
        assert_eq!(
            g.vault as u64,
            env.token_amount(env.vault),
            "accounting == real vault"
        );
        assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    };

    // (1) Over-budget in one shot must reject (amount > remaining domain budget).
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 0, 1_000_001)
            .is_err(),
        "withdraw > domain budget must reject"
    );
    conserve(&env);

    // (2) Partial withdraw succeeds and debits the remaining budget (and insurance).
    let (_d, _cu) = env
        .try_withdraw_insurance_domain_with_authority(&admin, 0, 600_000)
        .expect("partial domain withdraw ok");
    let g = env.market_state().1;
    assert_eq!(
        g.insurance_domain_budget[0], 400_000,
        "budget debited to 400k"
    );
    assert_eq!(g.insurance, 400_000, "insurance debited to 400k");
    conserve(&env);

    // (3) A second withdraw exceeding the NEW remaining budget must reject (no double-drain).
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 0, 500_000)
            .is_err(),
        "withdraw > remaining (400k) must reject — no double-drain"
    );
    let g = env.market_state().1;
    assert_eq!(
        g.insurance_domain_budget[0], 400_000,
        "rejected withdraw left budget intact"
    );
    conserve(&env);

    // (4) Draining exactly the remainder succeeds; budget -> 0.
    env.try_withdraw_insurance_domain_with_authority(&admin, 0, 400_000)
        .expect("drain remainder ok");
    let g = env.market_state().1;
    assert_eq!(g.insurance_domain_budget[0], 0, "budget fully drained");
    conserve(&env);

    // (5) Any further withdraw from the exhausted domain must reject.
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 0, 1)
            .is_err(),
        "withdraw from exhausted domain budget must reject"
    );
    conserve(&env);
}

// security.md sweep — uniform live insurance API (#6/#23/#57): asset 0 and permissionless assets 1..N
// both withdraw through the same asset-indexed tag. The signer must be that asset's insurance_operator,
// and the withdrawal is bounded to that asset's own long+short insurance budget.
#[test]
fn v16_attack_live_insurance_asset_withdraw_uniform_for_asset0_and_permissionless_asset() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let creator = Keypair::new();
    let cp = creator.pubkey();

    env.update_market_init_fee_policy_with_cu(10);
    let (_fee_source, _cu) =
        env.activate_permissionless_asset_with_fee(&creator, 1, 1, 100, cp, cp, cp, cp, 10);

    // Permissionless create fee credits asset 0 insurance 5/5. Top up to exact 300/200.
    env.top_up_insurance_domain_with_authority(&admin, 0, 295);
    env.top_up_insurance_domain_with_authority(&admin, 1, 195);
    env.top_up_insurance_domain_with_authority(&creator, 2, 100);
    env.top_up_insurance_domain_with_authority(&creator, 3, 70);

    let g0 = env.market_state().1;
    assert_eq!(g0.insurance_domain_budget[0], 300);
    assert_eq!(g0.insurance_domain_budget[1], 200);
    assert_eq!(g0.insurance_domain_budget[2], 100);
    assert_eq!(g0.insurance_domain_budget[3], 70);

    assert!(
        env.try_withdraw_insurance_asset_with_authority(&creator, 0, 1)
            .is_err(),
        "permissionless asset creator must not operate asset-0 insurance"
    );
    assert!(
        env.try_withdraw_insurance_asset_with_authority(&admin, 1, 1)
            .is_err(),
        "marketauth must not operate a live permissionless asset's insurance"
    );

    let (asset0_dest, _cu0) = env
        .try_withdraw_insurance_asset_with_authority(&admin, 0, 450)
        .expect("asset-0 operator withdraws through unified tag");
    assert_eq!(env.token_amount(asset0_dest), 450);

    let (asset1_dest, _cu1) = env
        .try_withdraw_insurance_asset_with_authority(&creator, 1, 120)
        .expect("permissionless asset operator withdraws through unified tag");
    assert_eq!(env.token_amount(asset1_dest), 120);

    let g = env.market_state().1;
    assert_eq!(
        g.insurance_domain_budget[0], 0,
        "asset-0 withdraw drains long side first"
    );
    assert_eq!(g.insurance_domain_budget[1], 50);
    assert_eq!(
        g.insurance_domain_budget[2], 0,
        "asset-1 withdraw drains long side first"
    );
    assert_eq!(g.insurance_domain_budget[3], 50);
    assert_eq!(
        g.insurance, 100,
        "only each asset's remaining budget stays insured"
    );
    assert!(
        env.try_withdraw_insurance_asset_with_authority(&creator, 1, 51)
            .is_err(),
        "asset operator cannot withdraw beyond its own remaining long+short budget"
    );
    let g = env.market_state().1;
    assert_eq!(g.vault as u64, env.token_amount(env.vault));
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

#[test]
fn v16_program_live_and_resolved_insurance_withdrawals_share_one_finite_budget() {
    const DOMAIN_BUDGETS: [u128; 4] = [11, 13, 17, 19];
    const FUNDED: u128 = 60;
    const LIVE_WITHDRAW: u128 = 7;
    const TERMINAL_REMAINING: u128 = FUNDED - LIVE_WITHDRAW;

    // The plans cross both asset orders and split execution without a market-wide selector.
    let plans: Vec<(&str, Vec<(u16, u128)>)> = vec![
        ("asset-forward", vec![(0, 17), (1, 36)]),
        ("asset-reverse", vec![(1, 36), (0, 17)]),
        ("split-forward", vec![(0, 5), (1, 20), (0, 12), (1, 16)]),
        ("split-reverse", vec![(1, 16), (0, 12), (1, 20), (0, 5)]),
    ];

    for (world, plan) in plans {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
        let admin = env.admin.insecure_clone();
        for (domain, amount) in DOMAIN_BUDGETS.into_iter().enumerate() {
            env.top_up_insurance_domain_with_authority(&admin, domain as u16, amount);
        }
        let funded = env.market_state().1;
        assert_eq!(funded.insurance, FUNDED, "{world}: aggregate funding");
        assert_eq!(funded.vault, FUNDED, "{world}: engine funding");
        assert_eq!(env.token_amount(env.vault), FUNDED as u64);

        let (live_destination, live_cu) = env
            .try_withdraw_insurance_asset_with_authority(&admin, 0, LIVE_WITHDRAW)
            .expect("healthy live asset route must consume its finite budget");
        assert_cu_within(
            &format!("{world}: live insurance withdrawal"),
            live_cu,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(env.token_amount(live_destination), LIVE_WITHDRAW as u64);
        let after_live = env.market_state().1;
        assert_eq!(after_live.insurance, TERMINAL_REMAINING);
        assert_eq!(after_live.vault, TERMINAL_REMAINING);
        assert_eq!(after_live.insurance_domain_budget[0], 4);
        assert_eq!(after_live.insurance_domain_budget[1], 13);

        env.resolve();
        let mut terminal_paid = 0u128;
        for (asset_index, amount) in plan {
            let (destination, cu) = env
                .try_withdraw_insurance_asset_with_authority(&admin, asset_index, amount)
                .expect("resolved asset route must consume the current domain budget");
            assert_cu_within(
                &format!("{world}: terminal insurance withdrawal"),
                cu,
                CUSTODY_CU_LIMIT,
            );
            assert_eq!(env.token_amount(destination), amount as u64);
            terminal_paid += amount;

            let group = env.market_state().1;
            let domain_remaining: u128 = group.insurance_domain_budget[..4].iter().sum();
            let expected_remaining = TERMINAL_REMAINING - terminal_paid;
            assert_eq!(group.insurance, expected_remaining, "{world}: insurance");
            assert_eq!(group.vault, expected_remaining, "{world}: engine vault");
            assert_eq!(
                domain_remaining, expected_remaining,
                "{world}: domain census"
            );
            assert_eq!(env.token_amount(env.vault), expected_remaining as u64);
        }

        assert_eq!(terminal_paid, TERMINAL_REMAINING, "{world}: exact drain");
        let exhausted = env.market_state().1;
        assert_eq!(exhausted.insurance, 0);
        assert_eq!(exhausted.vault, 0);
        assert!(exhausted.insurance_domain_budget[..4]
            .iter()
            .all(|amount| *amount == 0));

        // A mode transition cannot reset allowance or make the same insurance atom withdrawable
        // twice.
        let market_before = env.svm.get_account(&env.market).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        assert!(env
            .try_withdraw_insurance_asset_with_authority(&admin, 0, 1)
            .is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }
}

// Live and resolved insurance withdrawal is uniformly asset-scoped through tag 57. The old
// asset-0-only rate-limit tag, its policy-update tag, and the market-wide terminal tag must reject
// raw instruction bytes without mutating state.
#[test]
fn v16_attack_removed_limited_insurance_tags_reject_without_mutation() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 0, 1_000);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest = env.token_account(admin.pubkey(), 0);

    let mut old_limited = vec![23u8];
    old_limited.extend_from_slice(&100u128.to_le_bytes());
    env.svm.expire_blockhash();
    let limited = send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: env.program_id,
            data: old_limited,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
        &[&admin],
    );
    assert!(limited.is_err(), "old tag 23 must reject");

    let mut old_policy = vec![33u8];
    old_policy.extend_from_slice(&5_000u16.to_le_bytes());
    old_policy.push(0);
    old_policy.extend_from_slice(&10u64.to_le_bytes());
    env.svm.expire_blockhash();
    let policy = send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: env.program_id,
            data: old_policy,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
        },
        &[&admin],
    );
    assert!(policy.is_err(), "old tag 33 must reject");

    let mut old_market_wide = vec![41u8];
    old_market_wide.extend_from_slice(&100u128.to_le_bytes());
    env.svm.expire_blockhash();
    let market_wide = send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: env.program_id,
            data: old_market_wide,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
        &[&admin],
    );
    assert!(market_wide.is_err(), "removed tag 41 must reject");

    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.token_amount(dest), 0);
}

// Live insurance withdrawal must reject while insurance protects unresolved loss, but a sticky
// record that an earlier bankruptcy completed cannot permanently strand the remaining domain
// budget or prevent an empty Recovery asset from restarting. The public INV-073 Recovery route
// establishes reachability of that history-only state; this focused policy matrix isolates the
// wrapper discriminants. Threshold stress and stale loss state reject with exact ledger frames,
// while history alone admits one exact withdrawal and remains set for audit.
#[test]
fn v16_live_insurance_withdraw_uses_active_loss_not_bankruptcy_history() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 24);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100_000_000);
    env.enable_live_insurance_withdrawal();
    env.top_up_insurance(1_000_000);
    env.top_up_insurance_domain_with_authority(&env.admin.insecure_clone(), 0, 1_000_000);
    let admin = env.admin.insecure_clone();

    // Sanity: on a healthy, flat, lag-free market the live asset withdrawal succeeds — so any rejection
    // below is caused specifically by the stress/h-lock flag, not by some unrelated precondition.
    env.svm.expire_blockhash();
    env.try_withdraw_insurance_asset_with_authority(&admin, 0, 100)
        .expect("flat healthy live insurance withdrawal must succeed");

    // Each active "insurance still protecting loss" flag independently blocks the withdrawal.
    let cases: [(&str, fn(&mut MarketGroupV16, bool)); 2] = [
        ("threshold_stress_active", |g, v| {
            g.threshold_stress_active = v
        }),
        ("loss_stale_active", |g, v| g.loss_stale_active = v),
    ];
    for (label, set) in cases {
        env.mutate_market(|_cfg, group| set(group, true));
        let before = env.market_state().1;
        env.svm.expire_blockhash();
        let r = env.try_withdraw_insurance_asset_with_authority(&admin, 0, 100);
        assert!(
            r.is_err(),
            "live WithdrawInsuranceAsset must reject while {label} is set (insurance protecting loss)"
        );
        let after = env.market_state().1;
        assert_eq!(
            after.insurance, before.insurance,
            "rejected withdrawal under {label} must leave insurance untouched"
        );
        assert_eq!(
            after.insurance_domain_budget[0], before.insurance_domain_budget[0],
            "rejected withdrawal under {label} must leave the domain budget untouched"
        );
        // clear the flag so the next iteration tests its flag in isolation.
        env.mutate_market(|_cfg, group| set(group, false));
    }

    env.mutate_market(|_cfg, group| group.bankruptcy_hlock_active = true);
    let history_only = env.market_state().1;
    assert_eq!(history_only.negative_pnl_account_count, 0);
    assert_eq!(history_only.pending_domain_loss_barriers[0], 0);
    env.svm.expire_blockhash();
    let (destination, cu) = env
        .try_withdraw_insurance_asset_with_authority(&admin, 0, 100)
        .expect("settled bankruptcy history alone must not strand insurance-domain value");
    assert_cu_within(
        "history-only live insurance withdrawal",
        cu,
        CUSTODY_CU_LIMIT,
    );
    let after_history_withdraw = env.market_state().1;
    assert!(after_history_withdraw.bankruptcy_hlock_active);
    assert_eq!(
        after_history_withdraw.insurance,
        history_only.insurance - 100
    );
    assert_eq!(
        after_history_withdraw.insurance_domain_budget[0],
        history_only.insurance_domain_budget[0] - 100
    );
    assert_eq!(env.token_amount(destination), 100);
}

#[test]
fn v16_attack_unrelated_refresh_cannot_mask_loss_stale_insurance_gate() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.enable_live_insurance_withdrawal();
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 2, 100);

    let stale_long_owner = Keypair::new();
    let stale_short_owner = Keypair::new();
    let stale_long = env.create_portfolio(&stale_long_owner);
    let stale_short = env.create_portfolio(&stale_short_owner);
    env.deposit(&stale_long_owner, stale_long, 1_000_000_000);
    env.deposit(&stale_short_owner, stale_short, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &stale_long_owner,
        stale_long,
        &stale_short_owner,
        stale_short,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    let cranker_owner = Keypair::new();
    let cranker = env.create_portfolio(&cranker_owner);
    env.svm.warp_to_slot(3);
    let mut asset0_steps = 0usize;
    for _ in 0..3 {
        env.svm.expire_blockhash();
        if env
            .crank_if_actionable(
                cranker,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 3,
                    observations: crank_observations(0),
                },
            )
            .is_some()
        {
            asset0_steps += 1;
        } else {
            break;
        }
    }
    assert!(asset0_steps > 0, "asset 0 must make authenticated progress");
    env.svm.expire_blockhash();
    env.crank_if_actionable(
        cranker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(1),
        },
    );

    let before_mask = env.market_state().1;
    assert!(before_mask.loss_stale_active);
    assert!(before_mask.assets[1].slot_last < before_mask.current_slot);
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 2, 10)
            .is_err(),
        "asset-1 live insurance is initially locked by its loss-stale exposure"
    );

    env.svm.expire_blockhash();
    env.crank_if_actionable(
        cranker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let after_unrelated_refresh = env.market_state().1;
    assert!(
        after_unrelated_refresh.assets[1].slot_last < after_unrelated_refresh.current_slot,
        "asset 1 remains locally loss-stale after the unrelated asset-0 refresh"
    );

    let withdraw = env.try_withdraw_insurance_domain_with_authority(&admin, 2, 10);
    assert!(
        withdraw.is_err(),
        "an unrelated refresh must not make asset-1 insurance withdrawable while asset 1 is loss-stale"
    );
    let after_withdraw_attempt = env.market_state().1;
    assert_eq!(
        after_withdraw_attempt.insurance_domain_budget[2],
        after_unrelated_refresh.insurance_domain_budget[2],
        "rejected stale-asset withdrawal must leave the domain budget untouched"
    );
    assert_eq!(
        after_withdraw_attempt.insurance, after_unrelated_refresh.insurance,
        "rejected stale-asset withdrawal must leave insurance untouched"
    );
}

#[test]
fn v16_attack_unexposed_target_move_cannot_grief_live_insurance_withdrawals() {
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_price_move_bps_per_slot: 24,
            ..V16CuMarketParams::default()
        },
        1,
    );
    env.activate_asset(1, 1, 100_000_000);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100_000_000);
    env.enable_live_insurance_withdrawal();
    env.top_up_insurance(1_000_000);
    env.top_up_insurance_domain_with_authority(&env.admin.insecure_clone(), 0, 1_000_000);

    let owner = Keypair::new();
    let flat_portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, flat_portfolio, 1_000_000);
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(1, 2, 90_000_000);
    env.crank(
        flat_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(1),
        },
    );
    let g = env.market_state().1;
    assert_eq!(
        g.assets[1].raw_oracle_target_price, g.assets[1].effective_price,
        "zero-OI asset catches effective price up to target immediately"
    );
    assert_eq!(g.assets[1].effective_price, 90_000_000);
    assert_eq!(
        g.assets[1].oi_eff_long_q, 0,
        "asset 1 is unexposed long side"
    );
    assert_eq!(
        g.assets[1].oi_eff_short_q, 0,
        "asset 1 is unexposed short side"
    );

    let admin = env.admin.insecure_clone();
    let asset = env.try_withdraw_insurance_asset_with_authority(&admin, 0, 100);
    assert!(
        asset.is_ok(),
        "unexposed lag on another asset must not DoS unrelated asset insurance withdrawal"
    );
}

#[test]
fn v16_bpf_resolved_terminal_insurance_drains_dynamic_domain_after_positions_close() {
    const USER_PRINCIPAL: u128 = 1_000;
    const INSURANCE_ATOMS: u128 = 100;

    let mut env = V16CuEnv::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    env.svm
        .airdrop(&insurance_operator.pubkey(), 1_000_000_000)
        .unwrap();
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
    );

    let insurance_source =
        env.top_up_insurance_domain_with_authority(&insurance_authority, 2, INSURANCE_ATOMS);
    assert_eq!(env.token_amount(insurance_source), 0);
    assert_eq!(u128::from(env.token_amount(env.vault)), INSURANCE_ATOMS);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, USER_PRINCIPAL);
    env.deposit(&short_owner, short_account, USER_PRINCIPAL);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(10);
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(1),
        },
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert!(
        group.loss_stale_active,
        "advancing SVM Clock must reproduce the live stale-loss gate"
    );
    assert_eq!(group.insurance_domain_budget[2], INSURANCE_ATOMS);

    let market_before_reject = env.svm.get_account(&env.market).unwrap();
    let vault_before_reject = env.svm.get_account(&env.vault).unwrap();
    let long_before_reject = env.svm.get_account(&long_account).unwrap();
    let short_before_reject = env.svm.get_account(&short_account).unwrap();
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&insurance_operator, 2, INSURANCE_ATOMS,)
            .is_err(),
        "live domain withdrawal remains blocked while loss-stale"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_reject
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_reject
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap(),
        long_before_reject
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap(),
        short_before_reject
    );
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        2 * USER_PRINCIPAL + INSURANCE_ATOMS
    );

    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.assets[1].oi_eff_long_q, 0);
    assert_eq!(group.assets[1].oi_eff_short_q, 0);

    let long_dest = env.withdraw(&long_owner, long_account, USER_PRINCIPAL);
    let short_dest = env.withdraw(&short_owner, short_account, USER_PRINCIPAL);
    assert_eq!(u128::from(env.token_amount(long_dest)), USER_PRINCIPAL);
    assert_eq!(u128::from(env.token_amount(short_dest)), USER_PRINCIPAL);
    let after_user_withdrawals = env.market_state().1;
    assert_eq!(after_user_withdrawals.c_tot, 0);
    assert_eq!(after_user_withdrawals.insurance, INSURANCE_ATOMS);
    assert_eq!(after_user_withdrawals.vault, INSURANCE_ATOMS);
    assert_eq!(
        u128::from(env.token_amount(env.vault)),
        INSURANCE_ATOMS,
        "both users recover exact principal before insurance authority value can leave"
    );
    env.close_portfolio_with_cu(&long_owner, long_account);
    env.close_portfolio_with_cu(&short_owner, short_account);

    env.resolve();
    let (insurance_dest, _) =
        env.withdraw_terminal_insurance_with_authority(&insurance_authority, 1, INSURANCE_ATOMS);
    assert_eq!(
        u128::from(env.token_amount(insurance_dest)),
        INSURANCE_ATOMS
    );
    assert_eq!(env.token_amount(env.vault), 0);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.insurance_domain_budget[2], 0);
    assert_eq!(
        u128::from(env.token_amount(long_dest))
            + u128::from(env.token_amount(short_dest))
            + u128::from(env.token_amount(insurance_dest)),
        2 * USER_PRINCIPAL + INSURANCE_ATOMS,
        "terminal value partition returns user principal before exact residual insurance"
    );

    env.close_slab_with_cu();
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    assert!(market_data.iter().all(|b| *b == 0));
}
