//! INV-025 - exact stock reconciliation.
//!
//! This file owns a compact public-route stock ledger for the wrapper boundary.
//! After each successful value-moving instruction it independently tracks the SPL
//! vault atoms and compares them with `MarketGroupV16::vault`, then checks senior
//! stock lower bounds and the backing bucket/source-credit mirror for the touched
//! domain. Rejected value-moving instructions must roll back both program state and
//! custody exactly.

use super::*;

fn fresh_backing_atoms(group: &MarketGroupV16) -> u128 {
    group
        .source_credit
        .iter()
        .map(|source| source.fresh_reserved_backing_num / BOUND_SCALE)
        .sum()
}

fn assert_exact_stock(env: &V16CuEnv, expected_vault_atoms: u128, label: &str) {
    let (_, group) = env.market_state();
    assert_eq!(
        group.vault, expected_vault_atoms,
        "{label}: market vault stock matches independent ledger",
    );
    assert_eq!(
        env.token_amount(env.vault),
        u64::try_from(expected_vault_atoms).expect("test vault fits u64"),
        "{label}: SPL vault matches market vault stock",
    );
    assert!(
        group.vault >= group.c_tot + group.insurance + fresh_backing_atoms(&group),
        "{label}: vault covers capital, insurance, and fresh backing stocks",
    );
}

fn assert_domain_backing_mirror(env: &V16CuEnv, domain: usize, amount_atoms: u128, label: &str) {
    let (_, group) = env.market_state();
    let scaled = amount_atoms
        .checked_mul(BOUND_SCALE)
        .expect("test backing scale fits");
    let bucket = group.source_backing_buckets[domain];
    assert_eq!(
        bucket.fresh_unliened_backing_num, scaled,
        "{label}: fresh bucket amount matches expected scaled atoms",
    );
    assert_eq!(
        group.source_credit[domain].fresh_reserved_backing_num, scaled,
        "{label}: source-credit fresh reserve mirrors the bucket",
    );
}

#[test]
fn v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    assert_exact_stock(&env, 0, "genesis");

    env.deposit(&owner, portfolio, 100_000);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), 100_000);
    assert_exact_stock(&env, 100_000, "after user deposit");

    let withdraw_dest = env.withdraw(&owner, portfolio, 40_000);
    assert_eq!(env.token_amount(withdraw_dest), 40_000);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), 60_000);
    assert_exact_stock(&env, 60_000, "after user withdraw");

    env.top_up_insurance(7_000);
    assert_eq!(env.market_state().1.insurance, 7_000);
    assert_exact_stock(&env, 67_000, "after insurance top-up");

    let (insurance_dest, _) = env.withdraw_insurance_with_cu(2_000);
    assert_eq!(env.token_amount(insurance_dest), 2_000);
    assert_eq!(env.market_state().1.insurance, 5_000);
    assert_exact_stock(&env, 65_000, "after insurance withdraw");

    env.top_up_backing_bucket(0, 11_000, 100);
    assert_domain_backing_mirror(&env, 0, 11_000, "after backing top-up");
    assert_exact_stock(&env, 76_000, "after backing top-up");

    let backing_dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(backing_dest, 0, 4_000);
    assert_eq!(env.token_amount(backing_dest), 4_000);
    assert_domain_backing_mirror(&env, 0, 7_000, "after backing withdraw");
    assert_exact_stock(&env, 72_000, "after backing withdraw");

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&backing_dest).unwrap();
    let admin = env.admin.insecure_clone();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            amount: 7_001,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "over-withdraw of backing stock must reject",
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.svm.get_account(&backing_dest).unwrap(), dest_before);
    assert_domain_backing_mirror(&env, 0, 7_000, "after rejected over-withdraw");
    assert_exact_stock(&env, 72_000, "after rejected over-withdraw");
}

#[test]
fn v16_host_write_market_round_trips_source_fresh_backing_total() {
    let mut data = init_host_market_data_for_serializer_probe();
    let (cfg, mut group) = state::read_market(&data).unwrap();
    let amount = 37u128
        .checked_mul(BOUND_SCALE)
        .expect("probe amount within bound scale");
    let domain = 1usize;

    group.vault = group.vault.checked_add(37).unwrap();
    group.source_backing_buckets[domain] = percolator::BackingBucketV16 {
        market_id: group.assets[0].market_id,
        fresh_unliened_backing_num: amount,
        expiry_slot: 10,
        status: BackingBucketStatusV16::Fresh,
        ..percolator::BackingBucketV16::EMPTY
    };
    group.source_credit[domain].fresh_reserved_backing_num = amount;

    state::write_market(&mut data, &cfg, &group).unwrap();
    assert_eq!(
        market_group_header_bytes(&data)
            .source_fresh_backing_total_num
            .get(),
        amount,
        "host write_market must serialize the fresh backing aggregate used by engine residual math"
    );
}

#[test]
fn v16_bpf_accounting_ledger_tags_are_bounded_and_update_state() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    let (backing_source, top_up_cu) =
        env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    assert_cu_within(
        "TopUpBackingBucket ledger init",
        top_up_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(backing_source), 0);

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings = 30;
        group.vault += 30;
    });
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 130);

    let sync_cu = env.sync_backing_domain_ledger_with_cu(ledger, 1);
    assert_cu_within("SyncBackingDomainLedger", sync_cu, CUSTODY_CU_LIMIT);
    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_backing_domain_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 100);
    assert_eq!(
        ledger_state.residual_received_atoms(),
        0,
        "principal top-up is not rewardable residual"
    );
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 30);
    assert_eq!(ledger_state.total_earnings_atoms, 30);
    assert_eq!(
        ledger_state.residual_received_atoms(),
        0,
        "utilization earnings are not rewardable residual"
    );

    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    let withdraw_earnings_cu =
        env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(ledger, dest, 1, 20);
    assert_cu_within(
        "WithdrawBackingBucketEarnings",
        withdraw_earnings_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), 20);
    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_backing_domain_ledger(&ledger_data).unwrap();
    let (_, group) = env.market_state();
    assert_eq!(ledger_state.total_earnings_withdrawn_atoms, 20);
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 10);
    assert_eq!(
        ledger_state.residual_received_atoms(),
        0,
        "earnings withdrawal is not rewardable residual"
    );
    assert_eq!(group.source_backing_buckets[1].utilization_fee_earnings, 10);
    assert_eq!(group.vault, 110);

    let mut pnl_env = V16CuEnv::new();
    let pnl_ledger = pnl_env.backing_domain_ledger_account();
    pnl_env.top_up_backing_bucket_with_ledger_with_cu(pnl_ledger, 1, 40, 10);
    let owner = Keypair::new();
    let portfolio = pnl_env.create_portfolio(&owner);
    pnl_env.add_source_positive_pnl(portfolio, 1, 40);
    pnl_env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let convert_cu = pnl_env.convert_released_pnl_with_cu(&owner, portfolio, 40);
    assert_cu_within("ConvertReleasedPnl", convert_cu, CUSTODY_CU_LIMIT);
    let account = pnl_env.portfolio_state(portfolio);
    assert_eq!(account.capital.get(), 40);
    pnl_env.sync_backing_domain_ledger_with_cu(pnl_ledger, 1);
    let ledger_data = pnl_env.svm.get_account(&pnl_ledger).unwrap().data;
    let ledger_state = state::read_backing_domain_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.cumulative_loss_atoms, 40);
    assert_eq!(
        ledger_state.residual_received_atoms(),
        40,
        "farm-facing residual_received aliases the monotonic backing loss counter"
    );
    assert_eq!(
        ledger_state.residual_received_delta_since(0).unwrap(),
        40,
        "farm start/end snapshot delta is deterministic"
    );
    assert_eq!(ledger_state.last_observed_unavailable_principal_atoms, 40);

    let mut insurance_env = V16CuEnv::new();
    let insurance_ledger = insurance_env.insurance_ledger_account();
    let (_, insurance_top_up_cu) =
        insurance_env.top_up_insurance_with_ledger_with_cu(insurance_ledger, 100);
    assert_cu_within(
        "TopUpInsurance ledger init",
        insurance_top_up_cu,
        CUSTODY_CU_LIMIT,
    );
    let init_cu = insurance_env.sync_insurance_ledger_with_cu(insurance_ledger);
    assert_cu_within("SyncInsuranceLedger init", init_cu, CUSTODY_CU_LIMIT);
    let ledger_data = insurance_env
        .svm
        .get_account(&insurance_ledger)
        .unwrap()
        .data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 100);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 100);

    insurance_env.mutate_market(|_, group| {
        group.insurance += 30;
        group.vault += 30;
        group.insurance_domain_budget[0] += 15;
        group.insurance_domain_budget[1] += 15;
    });
    insurance_env.svm.expire_blockhash();
    let profit_cu = insurance_env.sync_insurance_ledger_with_cu(insurance_ledger);
    assert_cu_within("SyncInsuranceLedger profit", profit_cu, CUSTODY_CU_LIMIT);
    let ledger_data = insurance_env
        .svm
        .get_account(&insurance_ledger)
        .unwrap()
        .data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.cumulative_profit_atoms, 30);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 130);

    insurance_env.mutate_market(|_, group| {
        group.insurance -= 20;
        group.vault -= 20;
        group.insurance_domain_budget[0] -= 10;
        group.insurance_domain_budget[1] -= 10;
    });
    insurance_env.svm.expire_blockhash();
    let loss_cu = insurance_env.sync_insurance_ledger_with_cu(insurance_ledger);
    assert_cu_within("SyncInsuranceLedger loss", loss_cu, CUSTODY_CU_LIMIT);
    let ledger_data = insurance_env
        .svm
        .get_account(&insurance_ledger)
        .unwrap()
        .data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.cumulative_loss_atoms, 20);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 110);
}
