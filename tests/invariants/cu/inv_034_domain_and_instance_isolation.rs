//! INV-034 - Domain and instance isolation.
//!
//! Normative obligation: Value and liabilities cannot cross market instances or source domains without an explicit rule.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): cross-market maintenance
//! payer substitution and resolved top-up payout substitution. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_cross_instance_role_roster_is_source_complete() {
    let public_registry = include_str!("../public_instruction_coverage.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty() && !line.starts_with("tag\t"))
        .map(|line| {
            let fields = line.splitn(5, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 5, "malformed public registry row: {line}");
            (fields[0].parse::<u8>().expect("numeric tag"), fields[1])
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let test_sources = [
        include_str!("inv_034_domain_and_instance_isolation.rs"),
        include_str!("../stateful/inv_068_receipt_uniqueness_and_monotonic_topups.rs"),
    ];
    let mut roster = std::collections::BTreeMap::new();
    let mut status_counts = std::collections::BTreeMap::<&str, usize>::new();

    for line in include_str!("../inv_034_instance_role_coverage.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.splitn(6, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 6, "malformed INV-034 roster row: {line}");
        let tag = fields[0].parse::<u8>().expect("numeric tag");
        let variant = fields[1];
        let status = fields[2];
        let roles = fields[3];
        let evidence = fields[4];
        let gap = fields[5];
        assert_eq!(public_registry.get(&tag), Some(&variant));
        assert_ne!(roles, "-", "{variant} must classify its instance anchors");
        assert!(
            roster.insert(tag, variant).is_none(),
            "duplicate INV-034 tag {tag}"
        );
        *status_counts.entry(status).or_default() += 1;

        match status {
            "NO_MIXED_ROLE" => {
                assert_eq!(evidence, "-");
                assert_eq!(gap, "-");
            }
            "EXHAUSTIVE" => {
                assert_ne!(evidence, "-");
                assert_eq!(gap, "-");
            }
            "PARTIAL" => {
                assert_ne!(evidence, "-");
                assert_ne!(gap, "-");
            }
            "OPEN" => {
                assert_eq!(evidence, "-");
                assert_ne!(gap, "-");
            }
            other => panic!("unknown INV-034 role status {other}"),
        }
        if evidence != "-" {
            for test in evidence.split(',') {
                assert!(
                    test_sources
                        .iter()
                        .any(|source| source.contains(&format!("fn {test}"))),
                    "{variant} cites missing INV-034 evidence {test}"
                );
            }
        }
    }

    assert_eq!(
        roster, public_registry,
        "every public variant needs one row"
    );
    assert_eq!(status_counts.get("NO_MIXED_ROLE"), Some(&20));
    assert_eq!(status_counts.get("EXHAUSTIVE"), Some(&29));
    assert_eq!(status_counts.get("PARTIAL").copied().unwrap_or_default(), 0);
    assert_eq!(status_counts.get("OPEN").copied().unwrap_or_default(), 0);
}

fn init_authenticated_matcher_context_on_market(
    env: &mut V16CuEnv,
    matcher_program: Pubkey,
    market: Pubkey,
    owner: &Keypair,
    portfolio: Pubkey,
) -> (Pubkey, Pubkey) {
    env.ensure_signer_account(owner.pubkey());
    let context = Keypair::new();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::create_account(
            &env.payer.pubkey(),
            &context.pubkey(),
            1_000_000_000,
            MATCHER_CONTEXT_LEN as u64,
            &matcher_program,
        ),
        &[&context],
    )
    .expect("system-create authenticated matcher context");
    let delegate = matcher_delegate_key(
        &env.program_id,
        &market,
        &portfolio,
        &owner.pubkey(),
        &matcher_program,
        &context.pubkey(),
    );
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: matcher_program,
            accounts: vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new(context.pubkey(), false),
                AccountMeta::new_readonly(env.program_id, false),
                AccountMeta::new_readonly(market, false),
                AccountMeta::new_readonly(portfolio, false),
            ],
            data: vec![2],
        },
        &[owner],
    )
    .expect("initialize authenticated matcher context");
    (context.pubkey(), delegate)
}

#[allow(clippy::too_many_arguments)]
fn set_matcher_config_on_market(
    env: &mut V16CuEnv,
    market: Pubkey,
    owner: &Keypair,
    portfolio: Pubkey,
    matcher_program: Pubkey,
    matcher_context: Pubkey,
    matcher_delegate: Pubkey,
) -> Result<u64, String> {
    let portfolio_id = env.portfolio_id(portfolio);
    let expected_sequence = env.portfolio_matcher_sequence(portfolio);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SetMatcherConfig {
            portfolio_id,
            expected_sequence,
            enabled: 1,
            trade_fee_cap_bps: 10_000,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new_readonly(matcher_context, false),
            AccountMeta::new_readonly(matcher_delegate, false),
        ],
        &[owner],
    )
}

#[test]
fn v16_attack_set_matcher_config_rejects_cross_instance_capability_tuple() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(auth_matcher_program_path()).expect("read authenticated matcher SBF"),
    );
    let market_a = env.market;

    let owner_a = Keypair::new();
    let portfolio_a = env.create_portfolio(&owner_a);
    let (context_a, delegate_a) = init_authenticated_matcher_context_on_market(
        &mut env,
        matcher_program,
        market_a,
        &owner_a,
        portfolio_a,
    );

    let params = V16CuMarketParams::default();
    let (market_b, _vault_authority_b, _vault_b) =
        init_independent_market_same_mint(&mut env, params);
    let owner_b = Keypair::new();
    let portfolio_b = init_portfolio_on_market(
        &mut env,
        market_b,
        &owner_b,
        params.max_portfolio_assets as usize,
    );
    let (context_b, delegate_b) = init_authenticated_matcher_context_on_market(
        &mut env,
        matcher_program,
        market_b,
        &owner_b,
        portfolio_b,
    );

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let portfolio_a_before = env.svm.get_account(&portfolio_a).unwrap();
    let portfolio_b_before = env.svm.get_account(&portfolio_b).unwrap();
    let context_b_before = env.svm.get_account(&context_b).unwrap();
    let foreign_portfolio = set_matcher_config_on_market(
        &mut env,
        market_a,
        &owner_b,
        portfolio_b,
        matcher_program,
        context_b,
        delegate_b,
    );
    assert!(
        foreign_portfolio.is_err(),
        "market A must reject a portfolio initialized under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&portfolio_a).unwrap(),
        portfolio_a_before
    );
    assert_eq!(
        env.svm.get_account(&portfolio_b).unwrap(),
        portfolio_b_before
    );
    assert_eq!(env.svm.get_account(&context_b).unwrap(), context_b_before);

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_a_before = env.svm.get_account(&portfolio_a).unwrap();
    let context_b_before = env.svm.get_account(&context_b).unwrap();
    let foreign_delegate = set_matcher_config_on_market(
        &mut env,
        market_a,
        &owner_a,
        portfolio_a,
        matcher_program,
        context_b,
        delegate_b,
    );
    assert!(
        foreign_delegate.is_err(),
        "market B's delegate PDA must not install a capability under market A"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(
        env.svm.get_account(&portfolio_a).unwrap(),
        portfolio_a_before
    );
    assert_eq!(env.svm.get_account(&context_b).unwrap(), context_b_before);

    let control_a = set_matcher_config_on_market(
        &mut env,
        market_a,
        &owner_a,
        portfolio_a,
        matcher_program,
        context_a,
        delegate_a,
    )
    .expect("market-A matcher capability remains installable");
    assert_cu_within(
        "SetMatcherConfig market-A control",
        control_a,
        CUSTODY_CU_LIMIT,
    );
    let control_b = set_matcher_config_on_market(
        &mut env,
        market_b,
        &owner_b,
        portfolio_b,
        matcher_program,
        context_b,
        delegate_b,
    )
    .expect("market-B matcher capability remains installable");
    assert_cu_within(
        "SetMatcherConfig market-B control",
        control_b,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.portfolio_matcher_config(portfolio_a).matcher_delegate,
        delegate_a.to_bytes()
    );
    assert_eq!(
        env.portfolio_matcher_config(portfolio_b).matcher_delegate,
        delegate_b.to_bytes()
    );
}

#[test]
fn v16_attack_update_base_unit_mints_rejects_foreign_old_reserve() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let old_secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, old_secondary);

    let params = V16CuMarketParams::default();
    let (market_b, vault_authority_b, _primary_vault_b) =
        init_independent_market_same_mint(&mut env, params);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: old_secondary.to_bytes(),
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(old_secondary, false),
        ],
        &[&admin],
    )
    .expect("configure market B with the same secondary mint");

    let old_reserve_a =
        create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, old_secondary);
    let old_reserve_b =
        create_ata_for_test(&mut env.svm, &env.payer, vault_authority_b, old_secondary);
    assert_eq!(env.token_amount(old_reserve_a), 0);
    assert_eq!(env.token_amount(old_reserve_b), 0);

    let replacement = env.create_mint();
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let reserve_a_before = env.svm.get_account(&old_reserve_a).unwrap();
    let reserve_b_before = env.svm.get_account(&old_reserve_b).unwrap();
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: replacement.to_bytes(),
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(replacement, false),
            AccountMeta::new_readonly(old_reserve_b, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "market B's canonical reserve must not satisfy market A's old-reserve guard"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&old_reserve_a).unwrap(),
        reserve_a_before
    );
    assert_eq!(
        env.svm.get_account(&old_reserve_b).unwrap(),
        reserve_b_before
    );

    env.svm.expire_blockhash();
    let control = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: replacement.to_bytes(),
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(replacement, false),
            AccountMeta::new_readonly(old_reserve_a, false),
        ],
        &[&admin],
    )
    .expect("market A accepts its own empty old reserve");
    assert_cu_within(
        "UpdateBaseUnitMints same-market old reserve",
        control,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.market_state().0.secondary_collateral_mint,
        replacement.to_bytes()
    );
    let (_, market_b_group) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        market_b_group.vault, 0,
        "market B remains independently empty"
    );
}

#[test]
fn v16_attack_permissionless_activation_rejects_foreign_fee_vault() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);
    let params = V16CuMarketParams::default();
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let source = env.token_account(creator.pubkey(), FEE as u64);
    let activation_market_id = env.market_state().1.next_market_id;
    let activation = ProgInstruction::UpdateAssetLifecycle {
        action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
        asset_index: 1,
        market_id: activation_market_id,
        now_slot: 1,
        initial_price: 100,
        max_init_fee: FEE,
        insurance_authority: creator.pubkey().to_bytes(),
        insurance_operator: creator.pubkey().to_bytes(),
        backing_bucket_authority: creator.pubkey().to_bytes(),
        oracle_authority: creator.pubkey().to_bytes(),
    };

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        activation.clone(),
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        rejected.is_err(),
        "market B's canonical vault must not collect market A's activation fee"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);

    env.svm.expire_blockhash();
    let control = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        activation,
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    )
    .expect("market A accepts its canonical activation-fee vault");
    assert_cu_within(
        "UpdateAssetLifecycle same-market fee vault",
        control,
        CUSTODY_CU_LIMIT,
    );
    let (_, group_a) = env.market_state();
    assert_eq!(group_a.config.max_market_slots, 2);
    assert_eq!(group_a.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), FEE as u64);
    assert_eq!(env.token_amount(vault_b), 0);
    assert_eq!(group_a.vault, FEE);
    assert_eq!(group_a.insurance, FEE);
}

#[test]
fn v16_attack_sync_maintenance_rejects_cross_market_payer_substitution() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let foreign_owner = Keypair::new();
    let foreign_payer = env.create_portfolio(&foreign_owner);
    env.deposit(&foreign_owner, foreign_payer, 100_000_000);

    let params = V16CuMarketParams {
        maintenance_fee_per_slot: 58,
        ..V16CuMarketParams::default()
    };
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    let local_owner = Keypair::new();
    let local_payer = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_owner,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &local_owner,
        local_payer,
        100_000_000,
    );

    env.svm.warp_to_slot(10);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let foreign_before = env.svm.get_account(&foreign_payer).unwrap();
    let local_before = env.svm.get_account(&local_payer).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncMaintenanceFee { now_slot: 10 },
        vec![
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign_payer, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "SyncMaintenanceFee must reject a market-A payer under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&foreign_payer).unwrap(),
        foreign_before,
        "foreign payer is not charged, closed, or re-certified"
    );
    assert_eq!(
        env.svm.get_account(&local_payer).unwrap(),
        local_before,
        "local market-B payer is not touched by the rejected substitution"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let sync_cu = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncMaintenanceFee { now_slot: 10 },
        vec![
            AccountMeta::new(market_b, false),
            AccountMeta::new(local_payer, false),
        ],
        &[],
    )
    .expect("same-market SyncMaintenanceFee remains live");
    assert_cu_within(
        "SyncMaintenanceFee cross-market payer control",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );
    let local_after = state::read_portfolio(&env.svm.get_account(&local_payer).unwrap().data)
        .expect("market-B local payer");
    assert_eq!(local_after.last_fee_slot.get(), 10);
    assert_eq!(local_after.capital.get(), 100_000_000 - 580);
    let (_, market_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(market_b_after.insurance, 580);
    assert_eq!(market_b_after.vault as u64, env.token_amount(vault_b));
}

#[test]
fn v16_attack_sync_maintenance_rejects_cross_market_cranker_reward() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let payer_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(4_000);

    let market_b = Pubkey::new_unique();
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    )
    .expect("init market B");

    let foreign_cranker_owner = Keypair::new();
    env.ensure_signer_account(foreign_cranker_owner.pubkey());
    let foreign_cranker_portfolio = Pubkey::new_unique();
    env.svm
        .set_account(
            foreign_cranker_portfolio,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; env.portfolio_account_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(foreign_cranker_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign_cranker_portfolio, false),
        ],
        &[&foreign_cranker_owner],
    )
    .expect("init foreign cranker portfolio on market B");

    let payer_before = env.svm.get_account(&payer_portfolio).unwrap().data;
    let market_a_before = env.svm.get_account(&env.market).unwrap().data;
    let market_b_before = env.svm.get_account(&market_b).unwrap().data;
    let foreign_before = env
        .svm
        .get_account(&foreign_cranker_portfolio)
        .unwrap()
        .data;

    env.svm.warp_to_slot(10);
    env.svm.expire_blockhash();
    let rejected = env
        .try_sync_maintenance_fee_with_cu(payer_portfolio, Some(foreign_cranker_portfolio), 10)
        .expect_err("foreign-market cranker reward account must reject");
    assert!(
        rejected.contains("TransactionError") || rejected.contains("InstructionError"),
        "unexpected cross-market cranker rejection: {rejected}"
    );
    assert_eq!(
        env.svm.get_account(&payer_portfolio).unwrap().data,
        payer_before,
        "cross-market reward rejection must not charge the payer"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_a_before,
        "cross-market reward rejection must not mutate market A"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap().data,
        market_b_before,
        "cross-market reward rejection must not mutate market B"
    );
    assert_eq!(
        env.svm
            .get_account(&foreign_cranker_portfolio)
            .unwrap()
            .data,
        foreign_before,
        "cross-market reward rejection must not credit the foreign portfolio"
    );

    let local_cranker_owner = Keypair::new();
    let local_cranker_portfolio = env.create_portfolio(&local_cranker_owner);
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(payer_portfolio, Some(local_cranker_portfolio), 10);
    assert_eq!(env.portfolio_state(payer_portfolio).last_fee_slot.get(), 10);
    assert!(
        env.portfolio_state(local_cranker_portfolio).capital.get() > 0,
        "same-market cranker still receives the reward"
    );
}

// LoF/DoS sweep (PR135): ConvertReleasedPnl is a favorable owner action, so a stale health
// certificate must reject before any released-PnL mutation. The public crank route must then be
// enough to refresh the cert and let the owner convert, otherwise stale certs become a user DoS.

// security.md sweep - ConvertReleasedPnl market isolation (#2/#33/#44): owner authorization alone is not
// enough. A market-A portfolio with released source-backed PnL must not be convertible through market B's
// accounting slab, where it could consume B backing or corrupt B's senior capital counters.
#[test]
fn v16_attack_convert_released_pnl_rejects_cross_market_portfolio_substitution() {
    const RELEASED: u128 = 40;
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, RELEASED, 10_000);
    let foreign_owner = Keypair::new();
    let foreign = env.create_portfolio(&foreign_owner);
    env.add_source_positive_pnl(foreign, 1, RELEASED);
    env.crank(
        foreign,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    assert_eq!(
        env.portfolio_state(foreign)
            .provenance_header
            .market_group_id,
        env.market.to_bytes(),
        "foreign conversion target is genuinely bound to market A"
    );
    assert_eq!(
        env.portfolio_state(foreign).pnl.get(),
        RELEASED as i128,
        "foreign setup has released positive PnL before the substitution attempt"
    );

    let params = V16CuMarketParams::default();
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    top_up_backing_bucket_to_market(&mut env, market_b, vault_b, 1, RELEASED, 10_000);
    let local_owner = Keypair::new();
    let local = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_owner,
        params.max_portfolio_assets as usize,
    );
    add_source_positive_pnl_to_market(&mut env, market_b, local, 1, RELEASED);
    crank_portfolio_on_market(
        &mut env,
        market_b,
        local,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    assert_eq!(
        env.portfolio_state(local).provenance_header.market_group_id,
        market_b.to_bytes(),
        "control conversion target is genuinely bound to market B"
    );
    assert_eq!(
        env.portfolio_state(local).pnl.get(),
        RELEASED as i128,
        "control setup has released positive PnL on market B"
    );

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let foreign_before = env.svm.get_account(&foreign).unwrap();
    let local_before = env.svm.get_account(&local).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    env.svm.expire_blockhash();
    let foreign_portfolio_id = env.portfolio_id(foreign);
    let foreign_position_epoch = env.portfolio_position_epoch(foreign);
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id: foreign_portfolio_id,
            position_epoch: foreign_position_epoch,
            amount: RELEASED,
        },
        vec![
            AccountMeta::new(foreign_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign, false),
        ],
        &[&foreign_owner],
    );
    assert!(
        rejected.is_err(),
        "ConvertReleasedPnl must reject a market-A portfolio under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&foreign).unwrap(),
        foreign_before,
        "foreign portfolio is not converted or re-certified"
    );
    assert_eq!(
        env.svm.get_account(&local).unwrap(),
        local_before,
        "local market-B portfolio is not touched by the rejected substitution"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let local_portfolio_id = env.portfolio_id(local);
    let local_position_epoch = env.portfolio_position_epoch(local);
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id: local_portfolio_id,
            position_epoch: local_position_epoch,
            amount: RELEASED,
        },
        vec![
            AccountMeta::new(local_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(local, false),
        ],
        &[&local_owner],
    );
    assert!(
        ok.is_ok(),
        "same-market ConvertReleasedPnl succeeds: {ok:?}"
    );
    let local_after = env.portfolio_state(local);
    assert_eq!(
        local_after.capital.get(),
        RELEASED,
        "same-market conversion moves released PnL into local senior capital"
    );
    assert_eq!(local_after.pnl.get(), 0, "released PnL is consumed");
    let (_, market_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        market_b_after.c_tot, RELEASED,
        "market B senior capital increased only for its local portfolio"
    );
    assert_eq!(
        market_b_after.source_backing_buckets[1].consumed_liened_backing_num,
        RELEASED * BOUND_SCALE,
        "market B conversion consumes exactly its own source backing"
    );
    assert_eq!(
        market_b_after.vault as u64,
        env.token_amount(vault_b),
        "market B accounting still matches SPL custody"
    );
}

// full-interface sweep (cron29): CureAndCancelClose validates token accounts before the engine cure
// and transfers after the engine mutation. A market-A portfolio must not be curable through market B
// with a valid market-B vault, or the source transfer could fund one market while canceling and
// crediting an account bound to another.
#[test]
fn v16_attack_cure_rejects_cross_market_portfolio_before_transfer() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();

    let portfolio_a = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio_a, 100);
    env.seed_cancellable_close_progress(portfolio_a);

    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, V16CuMarketParams::default());

    let source = env.token_account_for_mint(env.mint, owner.pubkey(), 50);
    let portfolio_a_id = env.portfolio_id(portfolio_a);
    let portfolio_a_position_epoch = env.portfolio_position_epoch(portfolio_a);
    for (label, market, vault) in [
        ("foreign portfolio", market_b, vault_b),
        ("foreign vault", env.market, vault_b),
    ] {
        let market_a_before = env.svm.get_account(&env.market).unwrap();
        let market_b_before = env.svm.get_account(&market_b).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio_a).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_a_before = env.svm.get_account(&env.vault).unwrap();
        let vault_b_before = env.svm.get_account(&vault_b).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::CureAndCancelClose {
                portfolio_id: portfolio_a_id,
                position_epoch: portfolio_a_position_epoch,
                optional_deposit: 50,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(portfolio_a, false),
                AccountMeta::new(source, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        );
        assert!(rejected.is_err(), "CureAndCancelClose must reject {label}");
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
        assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
        assert_eq!(env.svm.get_account(&portfolio_a).unwrap(), portfolio_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
        assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    }

    env.svm.expire_blockhash();
    let control = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::CureAndCancelClose {
            portfolio_id: portfolio_a_id,
            position_epoch: portfolio_a_position_epoch,
            optional_deposit: 50,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio_a, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    )
    .expect("same-market CureAndCancelClose remains live");
    assert_cu_within("CureAndCancelClose control", control, CUSTODY_CU_LIMIT);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), 150);
    let account = env.portfolio_state(portfolio_a);
    assert_eq!(account.capital.get(), 150);
    assert!(close_progress(&account).canceled);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 150);
    assert_eq!(group.c_tot, 150);
}

// security.md sweep — ledger account binding (#44, F-VAULT-FRAG sibling): a backing-domain ledger is
// bound to (market_group, authority, domain). Passing a ledger under the WRONG domain must reject —
// no cross-domain earnings/accounting manipulation. (Contrast the vault, which is owner-only.)
#[test]
fn v16_attack_backing_ledger_domain_binding_enforced() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10); // ledger bound to domain 1
    env.top_up_backing_bucket(2, 100, 10); // make domain 2 valid and funded too.
                                           // sync the SAME ledger but claiming domain 2 -> must reject (domain mismatch).
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 2 },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );
    assert!(
        r.is_err(),
        "ledger used under the wrong domain must reject (binding enforced)"
    );

    // the spend path must also reject a wrong-domain ledger before paying earnings out of the vault.
    env.mutate_market(|_, group| {
        group.source_backing_buckets[2].utilization_fee_earnings = 40;
        group.vault += 40;
    });
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 240);
    let ledger_before = env.svm.get_account(&ledger).unwrap().data;
    let (_, g_before_spend) = env.market_state();
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let r_spend = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 2,
            market_id: g_before_spend.assets[1].market_id,
            authority_epoch: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    );
    assert!(
        r_spend.is_err(),
        "wrong-domain ledger must not authorize an earnings withdrawal"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no earnings paid with the wrong ledger"
    );
    assert_eq!(
        env.market_state().1.vault,
        g_before_spend.vault,
        "vault accounting unchanged"
    );
    assert_eq!(
        env.token_amount(env.vault),
        g_before_spend.vault as u64,
        "real vault unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap().data,
        ledger_before,
        "wrong-domain spend does not rewrite ledger"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[2].utilization_fee_earnings,
        40,
        "domain-2 earnings remain withdrawable"
    );

    // the correct domain still syncs.
    env.svm.expire_blockhash();
    let r_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );
    assert!(r_ok.is_ok(), "correct-domain sync works: {:?}", r_ok);
}

// full-interface sweep (cron25): a correctly-shaped backing ledger is scoped to its market group.
// Replaying market A's ledger against market B's funded backing bucket must not sync, withdraw
// earnings, mutate either market, or move market B vault tokens.
#[test]
fn v16_attack_backing_ledger_market_binding_enforced() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let ledger_a = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger_a, 1, 100, 10);
    let ledger_a_state =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger_a).unwrap().data).unwrap();
    assert_eq!(
        ledger_a_state.market_group,
        env.market.to_bytes(),
        "setup must bind ledger A to market A"
    );

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&admin],
    )
    .expect("init market B");
    let market_b_market_id = state::read_market(&env.svm.get_account(&market_b).unwrap().data)
        .unwrap()
        .1
        .assets[0]
        .market_id;

    let source_b = env.token_account(admin.pubkey(), 100);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: market_b_market_id,
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(source_b, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .expect("top up market-B backing bucket");
    assert_eq!(env.token_amount(source_b), 0, "market-B bucket was funded");
    {
        let mut market_b_account = env.svm.get_account(&market_b).unwrap();
        let (cfg, mut group) = state::read_market(&market_b_account.data).unwrap();
        group.source_backing_buckets[1].utilization_fee_earnings = 30;
        group.vault += 30;
        state::write_market(&mut market_b_account.data, &cfg, &group).unwrap();
        env.svm.set_account(market_b, market_b_account).unwrap();
    }
    env.set_token_account_amount(vault_b, env.mint, vault_authority_b, 130);

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let ledger_a_before = env.svm.get_account(&ledger_a).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();

    env.svm.expire_blockhash();
    let sync_wrong_market = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        sync_wrong_market.is_err(),
        "market B must reject a backing ledger initialized for market A"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    let bad_dest = env.token_account(admin.pubkey(), 0);
    let bad_dest_before = env.svm.get_account(&bad_dest).unwrap();
    env.svm.expire_blockhash();
    let withdraw_wrong_market = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id: market_b_market_id,
            authority_epoch: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_a, false),
            AccountMeta::new(bad_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_wrong_market.is_err(),
        "market B must not pay provider earnings through market A's ledger"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&bad_dest).unwrap(), bad_dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    assert_eq!(env.token_amount(bad_dest), 0);

    for (label, vault, optional_ledger) in [
        ("foreign ledger", vault_b, Some(ledger_a)),
        ("foreign vault", env.vault, None),
    ] {
        let mut accounts = vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(bad_dest, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        if let Some(ledger) = optional_ledger {
            accounts.push(AccountMeta::new(ledger, false));
        }
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::WithdrawBackingBucket {
                domain: 1,
                market_id: market_b_market_id,
                authority_epoch: 0,
                amount: 10,
            },
            accounts,
            &[&admin],
        );
        assert!(
            rejected.is_err(),
            "market B principal withdrawal must reject {label}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
        assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
        assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
        assert_eq!(env.svm.get_account(&bad_dest).unwrap(), bad_dest_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
        assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    }

    let ledger_b = env.backing_domain_ledger_account();
    env.svm.expire_blockhash();
    let sync_b = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        sync_b.is_ok(),
        "fresh market-B backing ledger syncs: {sync_b:?}"
    );
    let ledger_b_state =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger_b).unwrap().data).unwrap();
    assert_eq!(ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(ledger_b_state.total_principal_atoms, 0);
    assert_eq!(ledger_b_state.total_earnings_atoms, 0);
    assert_eq!(ledger_b_state.last_observed_bucket_earnings_atoms, 30);

    let good_dest = env.token_account(admin.pubkey(), 0);
    let market_b_ready = env.svm.get_account(&market_b).unwrap();
    let ledger_b_ready = env.svm.get_account(&ledger_b).unwrap();
    let good_dest_before = env.svm.get_account(&good_dest).unwrap();
    env.svm.expire_blockhash();
    let foreign_earnings_vault = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id: market_b_market_id,
            authority_epoch: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_b, false),
            AccountMeta::new(good_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        foreign_earnings_vault.is_err(),
        "market B earnings withdrawal must reject market A's canonical vault"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_ready);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&ledger_b).unwrap(), ledger_b_ready);
    assert_eq!(env.svm.get_account(&good_dest).unwrap(), good_dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let withdraw_b = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id: market_b_market_id,
            authority_epoch: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_b, false),
            AccountMeta::new(good_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_b.is_ok(),
        "same-market backing earnings withdraw works: {withdraw_b:?}"
    );
    assert_eq!(env.token_amount(good_dest), 10);
    let (_, group_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        group_b_after.source_backing_buckets[1].utilization_fee_earnings,
        20
    );
    assert_eq!(group_b_after.vault, 120);
    assert_eq!(env.token_amount(vault_b), 120);
    let ledger_b_after =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger_b).unwrap().data).unwrap();
    assert_eq!(ledger_b_after.total_earnings_withdrawn_atoms, 10);
    assert_eq!(ledger_b_after.last_observed_bucket_earnings_atoms, 20);

    let principal_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let principal_withdraw = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id: market_b_market_id,
            authority_epoch: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(principal_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        principal_withdraw.is_ok(),
        "same-market backing principal withdrawal works: {principal_withdraw:?}"
    );
    assert_eq!(env.token_amount(principal_dest), 10);
    let (_, final_group_b) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        final_group_b.source_backing_buckets[1].fresh_unliened_backing_num,
        90 * BOUND_SCALE
    );
    assert_eq!(final_group_b.vault, 110);
    assert_eq!(env.token_amount(vault_b), 110);
}

#[test]
fn v16_attack_insurance_ledger_authority_binding_enforced() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);

    let wrong_ledger = env.insurance_ledger_account();
    let wrong_authority = Keypair::new();
    let mut wrong_ledger_account = env.svm.get_account(&wrong_ledger).unwrap();
    state::init_insurance_ledger(
        &mut wrong_ledger_account.data,
        &state::InsuranceLedgerAccountV16 {
            market_group: env.market.to_bytes(),
            authority: wrong_authority.pubkey().to_bytes(),
            total_principal_atoms: 100,
            total_deposited_atoms: 100,
            total_withdrawn_atoms: 0,
            cumulative_profit_atoms: 0,
            cumulative_loss_atoms: 0,
            last_observed_insurance_atoms: 100,
        },
    )
    .expect("initialize wrong-authority insurance ledger");
    env.svm
        .set_account(wrong_ledger, wrong_ledger_account)
        .unwrap();

    let bad_dest = env.token_account(admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let ledger_before = env.svm.get_account(&wrong_ledger).unwrap().data;
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let bad = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(bad_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(wrong_ledger, false),
        ],
        &[&admin],
    );
    assert!(
        bad.is_err(),
        "wrong-authority insurance ledger must not authorize a domain withdrawal"
    );
    assert_eq!(
        env.token_amount(bad_dest),
        0,
        "no payout through wrong ledger"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "wrong-authority insurance ledger must not mutate market accounting"
    );
    assert_eq!(
        env.svm.get_account(&wrong_ledger).unwrap().data,
        ledger_before,
        "wrong-authority insurance ledger must not be rewritten"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "wrong-authority insurance ledger must not move vault tokens"
    );

    let correct_ledger = env.insurance_ledger_account();
    let good_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(good_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(correct_ledger, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "matching-authority ledger withdraw works: {ok:?}"
    );
    assert_eq!(env.token_amount(good_dest), 40);
    let ledger_state =
        state::read_insurance_ledger(&env.svm.get_account(&correct_ledger).unwrap().data)
            .expect("correct ledger initialized");
    assert_eq!(ledger_state.authority, admin.pubkey().to_bytes());
    assert_eq!(ledger_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 60);
    let group = env.market_state().1;
    assert_eq!(group.insurance_domain_budget[0], 60);
    assert_eq!(group.insurance, 60);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

// full-interface sweep (cron26): a correctly-shaped insurance ledger is scoped to its market group.
// Replaying market A's ledger against market B's insurance budget must not sync, withdraw domain
// insurance, rewrite either market, or move market B vault tokens.
#[test]
fn v16_attack_insurance_ledger_market_binding_enforced() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let ledger_a = env.insurance_ledger_account();
    env.top_up_insurance_with_ledger_with_cu(ledger_a, 100);
    let ledger_a_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger_a).unwrap().data).unwrap();
    assert_eq!(
        ledger_a_state.market_group,
        env.market.to_bytes(),
        "setup must bind insurance ledger A to market A"
    );
    assert_eq!(ledger_a_state.authority, admin.pubkey().to_bytes());

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&admin],
    )
    .expect("init market B");

    let source_b = env.token_account(admin.pubkey(), 100);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(source_b, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .expect("top up market-B domain insurance");
    assert_eq!(
        env.token_amount(source_b),
        0,
        "market-B insurance was funded"
    );

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let ledger_a_before = env.svm.get_account(&ledger_a).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();

    env.svm.expire_blockhash();
    let sync_wrong_market = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        sync_wrong_market.is_err(),
        "market B must reject an insurance ledger initialized for market A"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    let bad_dest = env.token_account(admin.pubkey(), 0);
    let bad_dest_before = env.svm.get_account(&bad_dest).unwrap();
    env.svm.expire_blockhash();
    let withdraw_wrong_market = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(bad_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_wrong_market.is_err(),
        "market B must not pay domain insurance through market A's ledger"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&bad_dest).unwrap(), bad_dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    assert_eq!(env.token_amount(bad_dest), 0);

    let ledger_b = env.insurance_ledger_account();
    env.svm.expire_blockhash();
    let sync_b = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        sync_b.is_ok(),
        "fresh market-B insurance ledger syncs: {sync_b:?}"
    );
    let ledger_b_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger_b).unwrap().data).unwrap();
    assert_eq!(ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(ledger_b_state.authority, admin.pubkey().to_bytes());
    assert_eq!(ledger_b_state.total_principal_atoms, 0);
    assert_eq!(ledger_b_state.last_observed_insurance_atoms, 100);

    let good_dest = env.token_account(admin.pubkey(), 0);
    let market_b_ready = env.svm.get_account(&market_b).unwrap();
    let ledger_b_ready = env.svm.get_account(&ledger_b).unwrap();
    let good_dest_before = env.svm.get_account(&good_dest).unwrap();
    env.svm.expire_blockhash();
    let foreign_vault = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(good_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        foreign_vault.is_err(),
        "market B must reject market A's canonical vault on domain insurance withdrawal"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_ready);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&ledger_b).unwrap(), ledger_b_ready);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    assert_eq!(env.svm.get_account(&good_dest).unwrap(), good_dest_before);

    env.svm.expire_blockhash();
    let withdraw_b = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 40,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(good_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_b.is_ok(),
        "same-market domain insurance withdraw works: {withdraw_b:?}"
    );
    assert_eq!(env.token_amount(good_dest), 40);
    let (_, group_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(group_b_after.insurance_domain_budget[0], 60);
    assert_eq!(group_b_after.insurance, 60);
    assert_eq!(group_b_after.vault, 60);
    assert_eq!(env.token_amount(vault_b), 60);
    let ledger_b_after =
        state::read_insurance_ledger(&env.svm.get_account(&ledger_b).unwrap().data).unwrap();
    assert_eq!(ledger_b_after.total_withdrawn_atoms, 40);
    assert_eq!(ledger_b_after.last_observed_insurance_atoms, 60);
}

// full-interface sweep (cron27): inbound value paths accept optional ledger accounts. A real
// market-A ledger must not be reusable on market-B top-ups, or funds could be pulled into one vault
// while another market's accounting ledger is updated. Rejections must happen before SPL transfer.
#[test]
fn v16_attack_topup_optional_ledgers_reject_cross_market_reuse() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let insurance_ledger_a = env.insurance_ledger_account();
    env.top_up_insurance_with_ledger_with_cu(insurance_ledger_a, 100);
    let backing_ledger_a = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(backing_ledger_a, 1, 100, 10);
    assert_eq!(
        state::read_insurance_ledger(&env.svm.get_account(&insurance_ledger_a).unwrap().data)
            .unwrap()
            .market_group,
        env.market.to_bytes()
    );
    assert_eq!(
        state::read_backing_domain_ledger(&env.svm.get_account(&backing_ledger_a).unwrap().data)
            .unwrap()
            .market_group,
        env.market.to_bytes()
    );

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&admin],
    )
    .expect("init market B");

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let insurance_ledger_a_before = env.svm.get_account(&insurance_ledger_a).unwrap();
    let backing_ledger_a_before = env.svm.get_account(&backing_ledger_a).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    let assert_core_unchanged = |env: &V16CuEnv| {
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
        assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
        assert_eq!(
            env.svm.get_account(&insurance_ledger_a).unwrap(),
            insurance_ledger_a_before
        );
        assert_eq!(
            env.svm.get_account(&backing_ledger_a).unwrap(),
            backing_ledger_a_before
        );
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
        assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    };

    let insurance_source = env.token_account(admin.pubkey(), 25);
    let insurance_source_before = env.svm.get_account(&insurance_source).unwrap();
    env.svm.expire_blockhash();
    let insurance_vault_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 1,
            market_id: 0,
            amount: 25,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(insurance_ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        insurance_vault_reject.is_err(),
        "market A TopUpInsurance must reject market B's canonical vault"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.svm.get_account(&insurance_source).unwrap(),
        insurance_source_before,
        "rejected foreign-vault insurance top-up must not pull source tokens"
    );

    env.svm.expire_blockhash();
    let insurance_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 25,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(insurance_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(insurance_ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        insurance_reject.is_err(),
        "market B TopUpInsurance must reject market A's insurance ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.svm.get_account(&insurance_source).unwrap(),
        insurance_source_before,
        "rejected insurance top-up must not pull source tokens"
    );

    let domain_source = env.token_account(admin.pubkey(), 30);
    let domain_source_before = env.svm.get_account(&domain_source).unwrap();
    env.svm.expire_blockhash();
    let domain_vault_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 1,
            market_id: 0,
            domain: 0,
            amount: 30,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(domain_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(insurance_ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        domain_vault_reject.is_err(),
        "market A TopUpInsuranceDomain must reject market B's canonical vault"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.svm.get_account(&domain_source).unwrap(),
        domain_source_before,
        "rejected foreign-vault domain top-up must not pull source tokens"
    );

    env.svm.expire_blockhash();
    let domain_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 30,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(domain_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(insurance_ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        domain_reject.is_err(),
        "market B TopUpInsuranceDomain must reject market A's insurance ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.svm.get_account(&domain_source).unwrap(),
        domain_source_before,
        "rejected domain top-up must not pull source tokens"
    );

    let backing_source = env.token_account(admin.pubkey(), 40);
    let backing_source_before = env.svm.get_account(&backing_source).unwrap();
    env.svm.expire_blockhash();
    let backing_vault_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 1,
            market_id: 0,
            domain: 1,
            amount: 40,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(backing_ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        backing_vault_reject.is_err(),
        "market A TopUpBackingBucket must reject market B's canonical vault"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.svm.get_account(&backing_source).unwrap(),
        backing_source_before,
        "rejected foreign-vault backing top-up must not pull source tokens"
    );

    env.svm.expire_blockhash();
    let backing_reject = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 40,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(backing_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(backing_ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        backing_reject.is_err(),
        "market B TopUpBackingBucket must reject market A's backing ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.svm.get_account(&backing_source).unwrap(),
        backing_source_before,
        "rejected backing top-up must not pull source tokens"
    );

    let insurance_ledger_b = env.insurance_ledger_account();
    let insurance_ok_source = env.token_account(admin.pubkey(), 25);
    env.svm.expire_blockhash();
    let insurance_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 25,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(insurance_ok_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(insurance_ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        insurance_ok.is_ok(),
        "same-market TopUpInsurance works: {insurance_ok:?}"
    );
    assert_eq!(env.token_amount(insurance_ok_source), 0);
    let insurance_ledger_b_state =
        state::read_insurance_ledger(&env.svm.get_account(&insurance_ledger_b).unwrap().data)
            .unwrap();
    assert_eq!(insurance_ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(insurance_ledger_b_state.total_principal_atoms, 25);
    assert_eq!(insurance_ledger_b_state.total_deposited_atoms, 25);
    assert_eq!(insurance_ledger_b_state.last_observed_insurance_atoms, 25);

    let domain_ledger_b = env.insurance_ledger_account();
    let domain_ok_source = env.token_account(admin.pubkey(), 30);
    env.svm.expire_blockhash();
    let domain_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 30,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(domain_ok_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(domain_ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        domain_ok.is_ok(),
        "same-market TopUpInsuranceDomain works: {domain_ok:?}"
    );
    assert_eq!(env.token_amount(domain_ok_source), 0);
    let domain_ledger_b_state =
        state::read_insurance_ledger(&env.svm.get_account(&domain_ledger_b).unwrap().data).unwrap();
    assert_eq!(domain_ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(domain_ledger_b_state.total_principal_atoms, 30);
    assert_eq!(domain_ledger_b_state.total_deposited_atoms, 30);

    let backing_ledger_b = env.backing_domain_ledger_account();
    let backing_ok_source = env.token_account(admin.pubkey(), 40);
    env.svm.expire_blockhash();
    let backing_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 40,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(backing_ok_source, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(backing_ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        backing_ok.is_ok(),
        "same-market TopUpBackingBucket works: {backing_ok:?}"
    );
    assert_eq!(env.token_amount(backing_ok_source), 0);
    let backing_ledger_b_state =
        state::read_backing_domain_ledger(&env.svm.get_account(&backing_ledger_b).unwrap().data)
            .unwrap();
    assert_eq!(backing_ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(backing_ledger_b_state.domain, 1);
    assert_eq!(backing_ledger_b_state.total_principal_atoms, 40);
    assert_eq!(backing_ledger_b_state.total_deposited_atoms, 40);

    let (_, group_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(group_b_after.vault, 95);
    assert_eq!(group_b_after.insurance, 55);
    assert_eq!(group_b_after.insurance_domain_budget[0], 42);
    assert_eq!(group_b_after.insurance_domain_budget[1], 13);
    assert_eq!(
        group_b_after.source_backing_buckets[1].fresh_unliened_backing_num,
        40 * BOUND_SCALE
    );
    assert_eq!(env.token_amount(vault_b), 95);
}

// full-interface sweep (cron28): resolved WithdrawInsuranceAsset shares the scoped route used live
// domain withdrawals. A real market-A insurance ledger must not authorize or record market-B terminal
// insurance withdrawals, even when the same authority controls both markets and market B is otherwise
// fully withdrawable.
#[test]
fn v16_attack_terminal_insurance_ledger_rejects_cross_market_reuse() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let ledger_a = env.insurance_ledger_account();
    env.top_up_insurance_with_ledger_with_cu(ledger_a, 100);
    let ledger_a_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger_a).unwrap().data).unwrap();
    assert_eq!(ledger_a_state.market_group, env.market.to_bytes());
    assert_eq!(ledger_a_state.authority, admin.pubkey().to_bytes());

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&admin],
    )
    .expect("init market B");

    let source_b = env.token_account(admin.pubkey(), 100);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(source_b, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .expect("fund market B insurance");
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
        ],
        &[&admin],
    )
    .expect("resolve market B");
    let (_, group_b) = state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(group_b.mode, percolator::MarketModeV16::Resolved);
    assert_eq!(group_b.insurance, 100);
    assert_eq!(group_b.vault, 100);
    let withdraw = || ProgInstruction::WithdrawInsuranceAsset {
        asset_index: 0,
        market_id: group_b.assets[0].market_id,
        authority_epoch: 0,
        amount: 40,
    };

    let dest = env.token_account(admin.pubkey(), 0);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    let ledger_a_before = env.svm.get_account(&ledger_a).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw(),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_a, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "market B resolved WithdrawInsuranceAsset must reject market A's insurance ledger"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap(),
        market_b_before,
        "rejected terminal withdraw must not debit market B"
    );
    assert_eq!(
        env.svm.get_account(&vault_b).unwrap(),
        vault_b_before,
        "rejected terminal withdraw must not move or close vault B"
    );
    assert_eq!(
        env.svm.get_account(&ledger_a).unwrap(),
        ledger_a_before,
        "rejected terminal withdraw must not rewrite market A's ledger"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected terminal withdraw must not pay the destination"
    );

    let ledger_b = env.insurance_ledger_account();
    let ledger_b_before = env.svm.get_account(&ledger_b).unwrap();
    env.svm.expire_blockhash();
    let foreign_vault = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw(),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        foreign_vault.is_err(),
        "market B resolved WithdrawInsuranceAsset must reject market A's canonical vault"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    assert_eq!(env.svm.get_account(&ledger_a).unwrap(), ledger_a_before);
    assert_eq!(env.svm.get_account(&ledger_b).unwrap(), ledger_b_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw(),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "same-market resolved WithdrawInsuranceAsset works: {ok:?}"
    );
    assert_eq!(env.token_amount(dest), 40);
    assert_eq!(env.token_amount(vault_b), 60);
    let (_, group_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(group_b_after.insurance, 60);
    assert_eq!(group_b_after.vault, 60);
    let ledger_b_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger_b).unwrap().data).unwrap();
    assert_eq!(ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(ledger_b_state.authority, admin.pubkey().to_bytes());
    assert_eq!(ledger_b_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_b_state.last_observed_insurance_atoms, 60);
}

// security.md sweep — terminal optional-ledger account-kind confusion (#35/#44): terminal
// Resolved WithdrawInsuranceAsset may rewrite an optional ledger before
// paying SPL tokens. A funded portfolio from another market must not be accepted as that ledger, or a
// wind-down helper could corrupt a user portfolio while draining terminal insurance.
#[test]
fn v16_attack_terminal_withdraw_insurance_rejects_portfolio_as_ledger() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();

    let victim = Keypair::new();
    let victim_portfolio = env.create_portfolio(&victim);
    env.deposit(&victim, victim_portfolio, 1_000);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let victim_portfolio_before = env.svm.get_account(&victim_portfolio).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();

    let (market_b, vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, V16CuMarketParams::default());
    let source_b = env.token_account(admin.pubkey(), 100);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(source_b, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .expect("fund market B terminal insurance");
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
        ],
        &[&admin],
    )
    .expect("resolve market B");
    let (_, group_b) = state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(group_b.mode, percolator::MarketModeV16::Resolved);
    assert_eq!(group_b.insurance, 100);
    assert_eq!(group_b.vault, 100);
    let withdraw = || ProgInstruction::WithdrawInsuranceAsset {
        asset_index: 0,
        market_id: group_b.assets[0].market_id,
        authority_epoch: 0,
        amount: 40,
    };

    let dest = env.token_account(admin.pubkey(), 0);
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw(),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(victim_portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "resolved WithdrawInsuranceAsset must reject a portfolio account as the optional ledger"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_a_before,
        "rejected terminal withdraw must not mutate the portfolio's source market"
    );
    assert_eq!(
        env.svm.get_account(&victim_portfolio).unwrap(),
        victim_portfolio_before,
        "rejected terminal withdraw must not rewrite the funded portfolio bytes or lamports"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_a_before,
        "rejected terminal withdraw must not touch market A custody"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap(),
        market_b_before,
        "rejected terminal withdraw must not debit market B"
    );
    assert_eq!(
        env.svm.get_account(&vault_b).unwrap(),
        vault_b_before,
        "rejected terminal withdraw must not move market B custody"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected terminal withdraw must not pay the destination"
    );

    env.svm.expire_blockhash();
    let market_alias = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw(),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(market_b, false),
        ],
        &[&admin],
    );
    assert!(
        market_alias.is_err(),
        "resolved WithdrawInsuranceAsset must reject the market account as the optional ledger"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap(),
        market_b_before,
        "terminal market-as-ledger rejection must not rewrite or debit market B"
    );
    assert_eq!(
        env.svm.get_account(&vault_b).unwrap(),
        vault_b_before,
        "terminal market-as-ledger rejection must not move market B custody"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "terminal market-as-ledger rejection must not pay the destination"
    );
    assert_eq!(
        env.svm.get_account(&victim_portfolio).unwrap(),
        victim_portfolio_before,
        "terminal market-as-ledger rejection still leaves unrelated portfolios untouched"
    );

    let ledger_b = env.insurance_ledger_account();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        withdraw(),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger_b, false),
        ],
        &[&admin],
    )
    .expect("same terminal withdraw with a real ledger succeeds");
    assert_eq!(env.token_amount(dest), 40);
    assert_eq!(env.token_amount(vault_b), 60);
    assert_eq!(
        env.svm.get_account(&victim_portfolio).unwrap(),
        victim_portfolio_before,
        "valid terminal withdraw on market B still leaves the unrelated portfolio untouched"
    );
    let ledger_b_state =
        state::read_insurance_ledger(&env.svm.get_account(&ledger_b).unwrap().data).unwrap();
    assert_eq!(ledger_b_state.market_group, market_b.to_bytes());
    assert_eq!(ledger_b_state.authority, admin.pubkey().to_bytes());
    assert_eq!(ledger_b_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_b_state.last_observed_insurance_atoms, 60);
}

// security.md sweep - RebalanceReduce market isolation (#2/#44): the owner signature is not enough.
// A portfolio initialized under market A must not be reducible through market B's slab, or a user could
// corrupt market B's OI/accounting while preserving the correct portfolio owner signer.
#[test]
fn v16_attack_rebalance_reduce_rejects_cross_market_portfolio_substitution() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let foreign_owner = Keypair::new();
    let foreign = env.create_portfolio(&foreign_owner);
    let foreign_short_owner = Keypair::new();
    let foreign_short = env.create_portfolio(&foreign_short_owner);
    env.deposit(&foreign_owner, foreign, 1_000_000);
    env.deposit(&foreign_short_owner, foreign_short, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &foreign_owner,
        foreign,
        &foreign_short_owner,
        foreign_short,
        POS_SCALE as i128,
        100,
        0,
    );
    assert_eq!(
        env.portfolio_state(foreign)
            .provenance_header
            .market_group_id,
        env.market.to_bytes(),
        "foreign reduce target is genuinely bound to market A"
    );

    let params = V16CuMarketParams::default();
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    let local_owner = Keypair::new();
    let local = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_owner,
        params.max_portfolio_assets as usize,
    );
    let local_short_owner = Keypair::new();
    let local_short = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_short_owner,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(&mut env, market_b, vault_b, &local_owner, local, 1_000_000);
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &local_short_owner,
        local_short,
        1_000_000,
    );
    env.svm.expire_blockhash();
    let local_trade_portfolio_id = env.portfolio_id(local);
    let local_short_trade_portfolio_id = env.portfolio_id(local_short);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: local_trade_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: local_short_trade_portfolio_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        vec![
            AccountMeta::new(local_owner.pubkey(), true),
            AccountMeta::new(local_short_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(local, false),
            AccountMeta::new(local_short, false),
        ],
        &[&local_owner, &local_short_owner],
    )
    .expect("open same-market B position");

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let foreign_before = env.svm.get_account(&foreign).unwrap();
    let local_before = env.svm.get_account(&local).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    let foreign_portfolio_id = env.portfolio_id(foreign);
    let foreign_position_epoch = env.portfolio_position_epoch(foreign);
    let local_portfolio_id = env.portfolio_id(local);
    let local_position_epoch = env.portfolio_position_epoch(local);
    let reduce_q = POS_SCALE / 2;
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::RebalanceReduce {
            portfolio_id: foreign_portfolio_id,
            position_epoch: foreign_position_epoch,
            asset_index: 0,
            reduce_q,
        },
        vec![
            AccountMeta::new(foreign_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign, false),
        ],
        &[&foreign_owner],
    );
    assert!(
        rejected.is_err(),
        "RebalanceReduce must reject a market-A portfolio under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&foreign).unwrap(),
        foreign_before,
        "foreign portfolio is not reduced or re-certified"
    );
    assert_eq!(
        env.svm.get_account(&local).unwrap(),
        local_before,
        "local market-B account is not touched by the rejected substitution"
    );
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::RebalanceReduce {
            portfolio_id: local_portfolio_id,
            position_epoch: local_position_epoch,
            asset_index: 0,
            reduce_q,
        },
        vec![
            AccountMeta::new(local_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(local, false),
        ],
        &[&local_owner],
    );
    assert!(ok.is_ok(), "same-market RebalanceReduce succeeds: {ok:?}");
    let local_after = state::read_portfolio(&env.svm.get_account(&local).unwrap().data)
        .expect("market-B local portfolio");
    assert!(
        active_leg_for_asset(&local_after, 0)
            .basis_pos_q
            .unsigned_abs()
            < POS_SCALE,
        "same-market reduce advanced the local position"
    );
    let (_, market_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        market_b_after.vault as u64,
        env.token_amount(vault_b),
        "market B accounting still matches SPL custody"
    );
    assert!(market_b_after.vault >= market_b_after.c_tot + market_b_after.insurance);
}

// security.md sweep - ForfeitRecoveryLeg market isolation (#2/#44/#48): the owner signature is not
// enough. In Recovery, forfeiting a leg realizes a loss and clears risk; a market-A portfolio must not be
// forfeit-able through market B's recovery slab even if market B has matching local OI.
#[test]
fn v16_attack_forfeit_recovery_leg_rejects_cross_market_portfolio_substitution() {
    let mut env = V16CuEnv::new();
    let foreign_owner = Keypair::new();
    let foreign = env.create_portfolio(&foreign_owner);
    let foreign_short_owner = Keypair::new();
    let foreign_short = env.create_portfolio(&foreign_short_owner);
    env.deposit(&foreign_owner, foreign, 10_000);
    env.deposit(&foreign_short_owner, foreign_short, 10_000);
    env.trade_with_cu(
        &foreign_owner,
        foreign,
        &foreign_short_owner,
        foreign_short,
        POS_SCALE as i128,
        100,
        0,
    );
    env.mutate_market(|_, group| {
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
    });
    assert_eq!(
        env.portfolio_state(foreign)
            .provenance_header
            .market_group_id,
        env.market.to_bytes(),
        "foreign forfeit target is genuinely bound to market A"
    );

    let params = V16CuMarketParams::default();
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    let local_owner = Keypair::new();
    let local = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_owner,
        params.max_portfolio_assets as usize,
    );
    let local_short_owner = Keypair::new();
    let local_short = init_portfolio_on_market(
        &mut env,
        market_b,
        &local_short_owner,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(&mut env, market_b, vault_b, &local_owner, local, 10_000);
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &local_short_owner,
        local_short,
        10_000,
    );
    env.svm.expire_blockhash();
    let local_trade_portfolio_id = env.portfolio_id(local);
    let local_short_trade_portfolio_id = env.portfolio_id(local_short);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: local_trade_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: local_short_trade_portfolio_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        vec![
            AccountMeta::new(local_owner.pubkey(), true),
            AccountMeta::new(local_short_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(local, false),
            AccountMeta::new(local_short, false),
        ],
        &[&local_owner, &local_short_owner],
    )
    .expect("open same-market B position");
    let mut market_b_account = env.svm.get_account(&market_b).unwrap();
    let (cfg_b, mut group_b) = state::read_market(&market_b_account.data).unwrap();
    group_b.mode = MarketModeV16::Recovery;
    group_b.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
    state::write_market(&mut market_b_account.data, &cfg_b, &group_b).unwrap();
    env.svm.set_account(market_b, market_b_account).unwrap();
    assert_eq!(
        env.portfolio_state(local).provenance_header.market_group_id,
        market_b.to_bytes(),
        "control forfeit target is genuinely bound to market B"
    );

    let foreign_portfolio_id = env.portfolio_id(foreign);
    let foreign_position_epoch = env.portfolio_position_epoch(foreign);
    let local_portfolio_id = env.portfolio_id(local);
    let local_position_epoch = env.portfolio_position_epoch(local);

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let foreign_before = env.svm.get_account(&foreign).unwrap();
    let local_before = env.svm.get_account(&local).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: foreign_portfolio_id,
            position_epoch: foreign_position_epoch,
            asset_index: 0,
            b_delta_budget: 1,
        },
        vec![
            AccountMeta::new(foreign_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign, false),
        ],
        &[&foreign_owner],
    );
    assert!(
        rejected.is_err(),
        "ForfeitRecoveryLeg must reject a market-A portfolio under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&foreign).unwrap(),
        foreign_before,
        "foreign portfolio is not forfeit-cleared"
    );
    assert_eq!(
        env.svm.get_account(&local).unwrap(),
        local_before,
        "local market-B account is not touched by the rejected substitution"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: local_portfolio_id,
            position_epoch: local_position_epoch,
            asset_index: 0,
            b_delta_budget: 1,
        },
        vec![
            AccountMeta::new(local_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(local, false),
        ],
        &[&local_owner],
    );
    assert!(
        ok.is_ok(),
        "same-market ForfeitRecoveryLeg succeeds: {ok:?}"
    );
    let local_after = env.portfolio_state(local);
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&local_after)),
        "same-market forfeit clears the local recovery leg"
    );
    assert_eq!(local_after.legs[0].basis_pos_q.get(), 0);
    let (_, market_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        market_b_after.assets[0].oi_eff_long_q, 0,
        "same-market forfeit clears market-B long OI"
    );
    assert_eq!(
        market_b_after.vault as u64,
        env.token_amount(vault_b),
        "market B accounting still matches SPL custody"
    );
}

// full-interface sweep (cron18) — cross-market portfolio substitution must NOT drain a foreign market's
// vault. A portfolio is bound to its market via provenance_header.market_group_id (= the market account
// key, stamped at InitPortfolio); validate_with_market (called on every fund path) rejects a mismatch.
// Attack: deposit into market A (crediting P_a.capital.get()), then withdraw the same amount from a SECOND
// market B's vault using P_a — would drain market B's other users if the binding were missing.
#[test]
fn v16_attack_cross_market_portfolio_cannot_drain_foreign_vault() {
    let mut env = V16CuEnv::new();
    // Market A: attacker's portfolio P_a holds real capital (bound to market A).
    let attacker = Keypair::new();
    let pa = env.create_portfolio(&attacker);
    env.deposit(&attacker, pa, 1_000_000);

    // --- Stand up an independent market B in the SAME svm, reusing the same mint. ---
    let params = V16CuMarketParams::default();
    let (market_b, vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);

    // Market B: a legit victim funds B's vault (1_000_000) via a portfolio bound to B.
    let victim = Keypair::new();
    let pb = init_portfolio_on_market(
        &mut env,
        market_b,
        &victim,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(&mut env, market_b, vault_b, &victim, pb, 1_000_000);
    assert_eq!(
        env.token_amount(vault_b),
        1_000_000,
        "market B vault funded"
    );

    let attack_source = env.token_account(attacker.pubkey(), 11);
    let pa_portfolio_id = env.portfolio_id(pa);
    let pa_sequence = env.portfolio_matcher_sequence(pa);
    for (label, market, vault) in [
        ("foreign portfolio", market_b, vault_b),
        ("foreign vault", env.market, vault_b),
    ] {
        let market_a_before = env.svm.get_account(&env.market).unwrap();
        let market_b_before = env.svm.get_account(&market_b).unwrap();
        let pa_before = env.svm.get_account(&pa).unwrap();
        let pb_before = env.svm.get_account(&pb).unwrap();
        let source_before = env.svm.get_account(&attack_source).unwrap();
        let vault_a_before = env.svm.get_account(&env.vault).unwrap();
        let vault_b_before = env.svm.get_account(&vault_b).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::Deposit {
                portfolio_id: pa_portfolio_id,
                expected_sequence: pa_sequence,
                amount: 11,
            },
            vec![
                AccountMeta::new(attacker.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(attack_source, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&attacker],
        );
        assert!(rejected.is_err(), "Deposit must reject {label}");
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
        assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
        assert_eq!(env.svm.get_account(&pa).unwrap(), pa_before);
        assert_eq!(env.svm.get_account(&pb).unwrap(), pb_before);
        assert_eq!(env.svm.get_account(&attack_source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
        assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    }

    env.svm.expire_blockhash();
    let deposit_control = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::Deposit {
            portfolio_id: pa_portfolio_id,
            expected_sequence: pa_sequence,
            amount: 11,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
            AccountMeta::new(attack_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    )
    .expect("same-market Deposit remains live");
    assert_cu_within(
        "Deposit same-market control",
        deposit_control,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(attack_source), 0);
    assert_eq!(env.portfolio_state(pa).capital.get(), 1_000_011);

    // --- ATTACK: withdraw from market B's vault using the market-A-bound portfolio P_a. ---
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, attacker.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let pa_withdraw_portfolio_id = env.portfolio_id(pa);
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::Withdraw {
            portfolio_id: pa_withdraw_portfolio_id,
            expected_sequence: 0,
            amount: 1_000_000,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(market_b, false), // foreign market B
            AccountMeta::new(pa, false),       // portfolio bound to market A
            AccountMeta::new(dest, false),
            AccountMeta::new(vault_b, false), // market B's vault
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        r.is_err(),
        "a portfolio bound to market A must NOT withdraw from market B's vault (provenance mismatch)"
    );
    assert_eq!(
        env.token_amount(vault_b),
        1_000_000,
        "market B vault must be intact after the cross-market drain attempt"
    );
    assert_eq!(env.token_amount(dest), 0, "attacker received nothing");

    // Make P_a otherwise closeable on its real market, then try to close it through market B.
    // If the market provenance check were missing, this could zero the market-A portfolio while
    // sweeping its rent to market B and leaving market A's materialized count stale.
    let (a_dest, _) = env.withdraw_with_cu(&attacker, pa, 1_000_011);
    assert_eq!(
        env.token_amount(a_dest),
        1_000_011,
        "market-A funds recovered before close probe"
    );
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        0,
        "P_a is flat and otherwise closeable"
    );
    let pa_before_close = env.svm.get_account(&pa).unwrap();
    let market_b_before_close = env.svm.get_account(&market_b).unwrap();
    env.svm.expire_blockhash();
    let pa_close_portfolio_id = env.portfolio_id(pa);
    let pa_close_sequence = env.portfolio_matcher_sequence(pa);
    let pa_close_position_epoch = env.portfolio_position_epoch(pa);
    let close_foreign = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ClosePortfolio {
            portfolio_id: pa_close_portfolio_id,
            expected_sequence: pa_close_sequence,
            position_epoch: pa_close_position_epoch,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(pa, false),
        ],
        &[&attacker],
    );
    assert!(
        close_foreign.is_err(),
        "market B must not close a market-A-bound portfolio"
    );
    assert_eq!(
        env.svm.get_account(&pa).unwrap(),
        pa_before_close,
        "foreign close leaves P_a intact"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap(),
        market_b_before_close,
        "foreign close sweeps no rent to market B and mutates no counter"
    );

    env.close_portfolio_with_cu(&attacker, pa);
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "real market can still close P_a"
    );
}

// full-interface sweep (cron/audit-scan-off) — every public trade entrypoint must reject a portfolio
// initialized under a different market, even though the account id and owner signer are otherwise
// correct. This proves the default deployed build does not rely on the engine's cfg-gated
// `audit-scan` helpers for trade provenance. The CPI probes use a faithful matcher reply, so the only
// rejected precondition is the foreign-market portfolio binding; the entire tx must roll back,
// including matcher ctx writes.
#[test]
fn v16_attack_trade_paths_reject_cross_market_portfolio_substitution() {
    let mut env = V16CuEnv::new();
    let market_a = env.market;
    let attacker = Keypair::new();
    let pa = env.create_portfolio(&attacker);
    env.deposit(&attacker, pa, 1_000_000);

    let params = V16CuMarketParams::default();
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    let victim = Keypair::new();
    let pb = init_portfolio_on_market(
        &mut env,
        market_b,
        &victim,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(&mut env, market_b, vault_b, &victim, pb, 1_000_000);
    assert_eq!(
        env.token_amount(vault_b),
        1_000_000,
        "market B vault is genuinely funded"
    );
    assert_eq!(
        env.portfolio_state(pa).provenance_header.market_group_id,
        env.market.to_bytes(),
        "attack account is bound to market A"
    );
    assert_eq!(
        env.portfolio_state(pb).provenance_header.market_group_id,
        market_b.to_bytes(),
        "counterparty is bound to market B"
    );

    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(auth_matcher_program_path()).expect("read authenticated matcher SBF"),
    );
    let (ctx_a, delegate_a) = init_authenticated_matcher_context_on_market(
        &mut env,
        matcher_program,
        market_a,
        &attacker,
        pa,
    );
    let (ctx, delegate) = init_authenticated_matcher_context_on_market(
        &mut env,
        matcher_program,
        market_b,
        &victim,
        pb,
    );
    set_matcher_config_on_market(
        &mut env,
        market_b,
        &victim,
        pb,
        matcher_program,
        ctx,
        delegate,
    )
    .expect("bind the market-B counterparty before cross-market probes");

    {
        let pa_portfolio_id = env.portfolio_id(pa);
        let pb_portfolio_id = env.portfolio_id(pb);
        let mut reject_atomically =
            |label: &str, ix: ProgInstruction, accounts: Vec<AccountMeta>, signers: &[&Keypair]| {
                let market_a_before = env.svm.get_account(&env.market).unwrap();
                let market_b_before = env.svm.get_account(&market_b).unwrap();
                let pa_before = env.svm.get_account(&pa).unwrap();
                let pb_before = env.svm.get_account(&pb).unwrap();
                let vault_b_before = env.svm.get_account(&vault_b).unwrap();
                let ctx_before = env.svm.get_account(&ctx).unwrap();
                env.svm.expire_blockhash();
                let result = send_tx(
                    &mut env.svm,
                    env.program_id,
                    &env.payer,
                    ix,
                    accounts,
                    signers,
                );
                assert!(
                    result.is_err(),
                    "{label} must reject a market-A portfolio under market B"
                );
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap(),
                    market_a_before,
                    "{label}: market A unchanged"
                );
                assert_eq!(
                    env.svm.get_account(&market_b).unwrap(),
                    market_b_before,
                    "{label}: market B unchanged"
                );
                assert_eq!(
                    env.svm.get_account(&pa).unwrap(),
                    pa_before,
                    "{label}: foreign portfolio unchanged"
                );
                assert_eq!(
                    env.svm.get_account(&pb).unwrap(),
                    pb_before,
                    "{label}: local counterparty unchanged"
                );
                assert_eq!(
                    env.svm.get_account(&vault_b).unwrap(),
                    vault_b_before,
                    "{label}: market B vault unchanged"
                );
                assert_eq!(
                    env.svm.get_account(&ctx).unwrap(),
                    ctx_before,
                    "{label}: matcher ctx writes rolled back"
                );
            };

        reject_atomically(
            "TradeNoCpi",
            ProgInstruction::TradeNoCpi {
                account_a_portfolio_id: pa_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id: pb_portfolio_id,
                account_b_position_epoch: 0,
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: POS_SCALE as i128,
                exec_price: 100,
                fee_bps: 100,
            },
            vec![
                AccountMeta::new(attacker.pubkey(), true),
                AccountMeta::new(victim.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(pb, false),
            ],
            &[&attacker, &victim],
        );
        reject_atomically(
            "BatchTradeNoCpi",
            ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id: pa_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id: pb_portfolio_id,
                account_b_position_epoch: 0,
                legs: vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: POS_SCALE as i128,
                    exec_price: 100,
                    fee_bps: 100,
                }],
            },
            vec![
                AccountMeta::new(attacker.pubkey(), true),
                AccountMeta::new(victim.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(pb, false),
            ],
            &[&attacker, &victim],
        );
        reject_atomically(
            "TradeCpi",
            ProgInstruction::TradeCpi {
                account_a_portfolio_id: pa_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id: pb_portfolio_id,
                account_b_position_epoch: 0,
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: POS_SCALE as i128,
                fee_bps: 100,
                limit_price: 0,
            },
            vec![
                AccountMeta::new(attacker.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(pb, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&attacker],
        );
        reject_atomically(
            "BatchTradeCpi",
            ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id: pa_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id: pb_portfolio_id,
                account_b_position_epoch: 0,
                legs: vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: POS_SCALE as i128,
                    fee_bps: 100,
                    limit_price: 0,
                }],
            },
            vec![
                AccountMeta::new(attacker.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(pb, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&attacker],
        );
    }

    let good_a = Keypair::new();
    let good_b = Keypair::new();
    let p_good_a = init_portfolio_on_market(
        &mut env,
        market_b,
        &good_a,
        params.max_portfolio_assets as usize,
    );
    let p_good_b = init_portfolio_on_market(
        &mut env,
        market_b,
        &good_b,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(&mut env, market_b, vault_b, &good_a, p_good_a, 1_000_000);
    deposit_to_market(&mut env, market_b, vault_b, &good_b, p_good_b, 1_000_000);
    env.svm.expire_blockhash();
    let p_good_a_id = env.portfolio_id(p_good_a);
    let p_good_b_id = env.portfolio_id(p_good_b);
    let direct_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: p_good_a_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: p_good_b_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 100,
        },
        vec![
            AccountMeta::new(good_a.pubkey(), true),
            AccountMeta::new(good_b.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(p_good_a, false),
            AccountMeta::new(p_good_b, false),
        ],
        &[&good_a, &good_b],
    );
    assert!(
        direct_ok.is_ok(),
        "same-market TradeNoCpi control must execute: {direct_ok:?}"
    );

    let cpi_taker = Keypair::new();
    let cpi_lp = Keypair::new();
    let p_cpi_taker = init_portfolio_on_market(
        &mut env,
        market_b,
        &cpi_taker,
        params.max_portfolio_assets as usize,
    );
    let p_cpi_lp = init_portfolio_on_market(
        &mut env,
        market_b,
        &cpi_lp,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &cpi_taker,
        p_cpi_taker,
        1_000_000,
    );
    deposit_to_market(&mut env, market_b, vault_b, &cpi_lp, p_cpi_lp, 1_000_000);
    let (ctx_ok, delegate_ok) = init_authenticated_matcher_context_on_market(
        &mut env,
        matcher_program,
        market_b,
        &cpi_lp,
        p_cpi_lp,
    );
    set_matcher_config_on_market(
        &mut env,
        market_b,
        &cpi_lp,
        p_cpi_lp,
        matcher_program,
        ctx_ok,
        delegate_ok,
    )
    .expect("set market-B LP matcher config");
    let cpi_taker_portfolio_id = env.portfolio_id(p_cpi_taker);
    let cpi_lp_account_portfolio_id = env.portfolio_id(p_cpi_lp);
    let mut reject_foreign_matcher_tuple = |label: &str, instruction: ProgInstruction| {
        let market_a_before = env.svm.get_account(&market_a).unwrap();
        let market_b_before = env.svm.get_account(&market_b).unwrap();
        let taker_before = env.svm.get_account(&p_cpi_taker).unwrap();
        let lp_before = env.svm.get_account(&p_cpi_lp).unwrap();
        let ctx_a_before = env.svm.get_account(&ctx_a).unwrap();
        let ctx_b_before = env.svm.get_account(&ctx_ok).unwrap();
        let vault_b_before = env.svm.get_account(&vault_b).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            instruction,
            vec![
                AccountMeta::new(cpi_taker.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(p_cpi_taker, false),
                AccountMeta::new(p_cpi_lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx_a, false),
                AccountMeta::new_readonly(delegate_a, false),
            ],
            &[&cpi_taker],
        );
        assert!(
            rejected.is_err(),
            "{label} must reject market A's matcher tuple under market B"
        );
        assert_eq!(env.svm.get_account(&market_a).unwrap(), market_a_before);
        assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
        assert_eq!(env.svm.get_account(&p_cpi_taker).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&p_cpi_lp).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&ctx_a).unwrap(), ctx_a_before);
        assert_eq!(env.svm.get_account(&ctx_ok).unwrap(), ctx_b_before);
        assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    };
    reject_foreign_matcher_tuple(
        "TradeCpi",
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: cpi_taker_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: cpi_lp_account_portfolio_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            fee_bps: 100,
            limit_price: 0,
        },
    );
    reject_foreign_matcher_tuple(
        "BatchTradeCpi",
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: cpi_taker_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: cpi_lp_account_portfolio_id,
            account_b_position_epoch: 0,
            legs: vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: POS_SCALE as i128,
                fee_bps: 100,
                limit_price: 0,
            }],
        },
    );

    env.svm.expire_blockhash();
    let single_cpi_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: cpi_taker_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: cpi_lp_account_portfolio_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            fee_bps: 100,
            limit_price: 0,
        },
        vec![
            AccountMeta::new(cpi_taker.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(p_cpi_taker, false),
            AccountMeta::new(p_cpi_lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx_ok, false),
            AccountMeta::new_readonly(delegate_ok, false),
        ],
        &[&cpi_taker],
    );
    assert!(
        single_cpi_ok.is_ok(),
        "same-market TradeCpi control must execute: {single_cpi_ok:?}"
    );

    let cpi_taker_position_epoch = env.portfolio_position_epoch(p_cpi_taker);
    let cpi_lp_position_epoch = env.portfolio_position_epoch(p_cpi_lp);
    env.svm.expire_blockhash();
    let batch_cpi_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: cpi_taker_portfolio_id,
            account_a_position_epoch: cpi_taker_position_epoch,
            account_b_portfolio_id: cpi_lp_account_portfolio_id,
            account_b_position_epoch: cpi_lp_position_epoch,
            legs: vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: POS_SCALE as i128,
                fee_bps: 100,
                limit_price: 0,
            }],
        },
        vec![
            AccountMeta::new(cpi_taker.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(p_cpi_taker, false),
            AccountMeta::new(p_cpi_lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx_ok, false),
            AccountMeta::new_readonly(delegate_ok, false),
        ],
        &[&cpi_taker],
    );
    assert!(
        batch_cpi_ok.is_ok(),
        "same-market BatchTradeCpi control must execute: {batch_cpi_ok:?}"
    );
}

// full-interface sweep (cron23): resolved payout is a terminal value-moving path. A portfolio bound
// to market A must not be closeable against resolved market B, even if its owner is named correctly and
// market B has enough vault value. Missing provenance here would pay market B's victims to the market-A
// portfolio owner or burn the market-A payout state.
#[test]
fn v16_attack_close_resolved_rejects_cross_market_portfolio_payout() {
    let mut env = V16CuEnv::new();

    let attacker = Keypair::new();
    let pa = env.create_portfolio(&attacker);
    env.deposit(&attacker, pa, 1_000_000);

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    )
    .expect("init market B");

    let victim = Keypair::new();
    env.ensure_signer_account(victim.pubkey());
    let pb = Pubkey::new_unique();
    env.svm
        .set_account(
            pb,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; env.portfolio_account_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(victim.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(pb, false),
        ],
        &[&victim],
    )
    .expect("init market-B victim portfolio");
    let pb_portfolio_id = env.portfolio_id(pb);
    let source_b = Pubkey::new_unique();
    env.svm
        .set_account(
            source_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, victim.pubkey(), 1_000_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::Deposit {
            portfolio_id: pb_portfolio_id,
            expected_sequence: 0,
            amount: 1_000_000,
        },
        vec![
            AccountMeta::new(victim.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(pb, false),
            AccountMeta::new(source_b, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&victim],
    )
    .expect("victim deposits into market B");
    assert_eq!(
        env.token_amount(vault_b),
        1_000_000,
        "market B vault funded"
    );

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
        ],
        &[&env.admin],
    )
    .expect("resolve market B");

    let attack_dest = env.token_account_for_mint(env.mint, attacker.pubkey(), 0);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let pa_before = env.svm.get_account(&pa).unwrap();
    let pb_before = env.svm.get_account(&pb).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(attacker.pubkey(), false),
            AccountMeta::new(market_b, false),
            AccountMeta::new(pa, false),
            AccountMeta::new(attack_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "market-B CloseResolved must reject a market-A-bound portfolio"
    );
    assert_eq!(
        env.token_amount(attack_dest),
        0,
        "attacker receives no market-B payout"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&pa).unwrap(),
        pa_before,
        "rejected cross-market payout leaves market-A portfolio byte-identical"
    );
    assert_eq!(env.svm.get_account(&pb).unwrap(), pb_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(
        env.svm.get_account(&vault_b).unwrap(),
        vault_b_before,
        "market-B vault remains intact"
    );

    let vault_probe_dest = env.token_account_for_mint(env.mint, victim.pubkey(), 0);
    let vault_probe_dest_before = env.svm.get_account(&vault_probe_dest).unwrap();
    env.svm.expire_blockhash();
    let foreign_vault = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim.pubkey(), false),
            AccountMeta::new(market_b, false),
            AccountMeta::new(pb, false),
            AccountMeta::new(vault_probe_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        foreign_vault.is_err(),
        "market-B CloseResolved must reject market A's canonical vault"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&pa).unwrap(), pa_before);
    assert_eq!(env.svm.get_account(&pb).unwrap(), pb_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);
    assert_eq!(
        env.svm.get_account(&vault_probe_dest).unwrap(),
        vault_probe_dest_before,
        "foreign-vault rejection must not pay or rewrite the victim destination"
    );

    let victim_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            victim_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, victim.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let victim_close = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim.pubkey(), false),
            AccountMeta::new(market_b, false),
            AccountMeta::new(pb, false),
            AccountMeta::new(victim_dest, false),
            AccountMeta::new(vault_b, false),
            AccountMeta::new_readonly(vault_authority_b, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        victim_close.is_ok(),
        "real market-B CloseResolved still pays: {victim_close:?}"
    );
    assert_eq!(env.token_amount(victim_dest), 1_000_000);

    env.resolve();
    let attacker_dest = env.close_resolved(&attacker, pa);
    assert_eq!(
        env.token_amount(attacker_dest),
        1_000_000,
        "market-A portfolio remains claimable on its real market"
    );
}

// full-interface sweep (cron24): ClaimResolvedPayoutTopup is intentionally unsigned so any cranker
// can finish a user's resolved payout. A market-A-bound receipt must not be claimable against market
// B's payout ledger and vault; otherwise a helper could drain one resolved market or burn another
// full-interface sweep (cron22) — PermissionlessCrank is intentionally unsigned for the target
// portfolio. A foreign-market portfolio with the same asset_index/market_id shape must still reject
// before any settlement/liquidation mutation; otherwise anyone could settle or liquidate a portfolio
// under the wrong market's oracle/accounting state.
#[test]
fn v16_attack_permissionless_crank_rejects_cross_market_target_portfolio() {
    let mut env = V16CuEnv::new();

    let market_b = Pubkey::new_unique();
    let vault_authority_b =
        Pubkey::find_program_address(&[b"vault", market_b.as_ref()], &env.program_id).0;
    let vault_b = canonical_vault_ata(vault_authority_b, env.mint);
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority_b, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    )
    .expect("init market B");

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    env.ensure_signer_account(long_owner.pubkey());
    env.ensure_signer_account(short_owner.pubkey());
    let long_b = Pubkey::new_unique();
    let short_b = Pubkey::new_unique();
    for portfolio in [long_b, short_b] {
        env.svm
            .set_account(
                portfolio,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; env.portfolio_account_len],
                    owner: env.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }
    for (owner, portfolio) in [(&long_owner, long_b), (&short_owner, short_b)] {
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("init market-B portfolio");

        let source = Pubkey::new_unique();
        env.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(env.mint, owner.pubkey(), 1_000_000),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        let portfolio_id = env.portfolio_id(portfolio);
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::Deposit {
                portfolio_id,
                expected_sequence: 0,
                amount: 1_000_000,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(market_b, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(vault_b, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .expect("deposit into market-B portfolio");
    }
    env.svm.expire_blockhash();
    let long_b_id = env.portfolio_id(long_b);
    let short_b_id = env.portfolio_id(short_b);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: long_b_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: short_b_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(long_b, false),
            AccountMeta::new(short_b, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("open a real market-B position");
    assert!(
        has_active_leg_for_asset(
            &state::read_portfolio(&env.svm.get_account(&long_b).unwrap().data).unwrap(),
            0
        ),
        "setup must create a non-vacuous market-B target portfolio"
    );

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let long_b_before = env.svm.get_account(&long_b).unwrap();
    let short_b_before = env.svm.get_account(&short_b).unwrap();
    let vault_a_before = env.svm.get_account(&env.vault).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();

    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_b, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "market A PermissionlessCrank must reject a market-B-bound target portfolio"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(
        env.svm.get_account(&long_b).unwrap(),
        long_b_before,
        "foreign target crank rejection leaves target portfolio byte-identical"
    );
    assert_eq!(env.svm.get_account(&short_b).unwrap(), short_b_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_a_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(long_b, false),
        ],
        &[],
    );
    assert!(
        ok.is_ok(),
        "same-market market-B crank remains live: {ok:?}"
    );
    let (_, group_b_after) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).unwrap();
    assert_eq!(
        group_b_after.current_slot, 5,
        "positive control advanced market B"
    );
    assert!(
        has_active_leg_for_asset(
            &state::read_portfolio(&env.svm.get_account(&long_b).unwrap().data).unwrap(),
            0
        ),
        "same-market crank does not erase the market-B position"
    );
}

// full-interface sweep (cron30): SwapSecondaryForPrimary must pin the secondary reserve to the
// current market's vault PDA. A valid secondary-mint token account owned by another market's vault PDA
// must not be usable as the reserve, or one market's base-unit authority could drain another market's
// secondary collateral while depositing primary into its own vault.
#[test]
fn v16_attack_swap_secondary_rejects_foreign_market_vault() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);

    let secondary_vault_a = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault_a,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let (market_b, vault_authority_b, primary_vault_b) =
        init_independent_market_same_mint(&mut env, V16CuMarketParams::default());
    let secondary_vault_b = canonical_vault_ata(vault_authority_b, secondary_mint);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: secondary_mint.to_bytes(),
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(secondary_mint, false),
        ],
        &[&admin],
    )
    .expect("configure market B secondary mint");
    env.svm
        .set_account(
            secondary_vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, vault_authority_b, 70),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let primary_source = env.token_account_for_mint(env.mint, admin.pubkey(), 10);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let foreign_primary_vault_before = env.svm.get_account(&primary_vault_b).unwrap();
    let secondary_vault_a_before = env.svm.get_account(&secondary_vault_a).unwrap();
    let secondary_vault_b_before = env.svm.get_account(&secondary_vault_b).unwrap();
    let source_before = env.svm.get_account(&primary_source).unwrap();
    let dest_before = env.svm.get_account(&secondary_dest).unwrap();

    let swap_with_vaults = |env: &mut V16CuEnv,
                            primary_vault: Pubkey,
                            secondary_vault: Pubkey|
     -> Result<u64, String> {
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::SwapSecondaryForPrimary {
                amount: 10,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(primary_source, false),
                AccountMeta::new(primary_vault, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };

    for (label, primary_vault, secondary_vault) in [
        ("foreign secondary reserve", env.vault, secondary_vault_b),
        (
            "foreign primary reserve",
            primary_vault_b,
            secondary_vault_a,
        ),
    ] {
        let rejected = swap_with_vaults(&mut env, primary_vault, secondary_vault);
        assert!(
            rejected.is_err(),
            "SwapSecondaryForPrimary must reject {label}"
        );
        assert_eq!(env.svm.get_account(&primary_source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&secondary_dest).unwrap(), dest_before);
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            primary_vault_before
        );
        assert_eq!(
            env.svm.get_account(&primary_vault_b).unwrap(),
            foreign_primary_vault_before
        );
        assert_eq!(
            env.svm.get_account(&secondary_vault_a).unwrap(),
            secondary_vault_a_before
        );
        assert_eq!(
            env.svm.get_account(&secondary_vault_b).unwrap(),
            secondary_vault_b_before
        );
    }

    let primary_vault_a = env.vault;
    let ok = swap_with_vaults(&mut env, primary_vault_a, secondary_vault_a);
    assert!(
        ok.is_ok(),
        "same-market secondary reserve swap succeeds: {ok:?}"
    );
    assert_eq!(env.token_amount(primary_source), 0);
    assert_eq!(env.token_amount(env.vault), 10);
    assert_eq!(env.token_amount(secondary_dest), 10);
    assert_eq!(env.token_amount(secondary_vault_a), 40);
    assert_eq!(env.token_amount(secondary_vault_b), 70);
}

// full-interface sweep (cron32): CloseSlab must bind both primary and optional secondary reserves to
// the current market's vault PDA. Either canonical reserve from another market must reject before
// primary dust is swept or either vault/market is closed.
#[test]
fn v16_attack_close_slab_rejects_foreign_market_vaults() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let secondary_vault_a = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault_a,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let (market_b, vault_authority_b, primary_vault_b) =
        init_independent_market_same_mint(&mut env, V16CuMarketParams::default());
    let secondary_vault_b = canonical_vault_ata(vault_authority_b, secondary_mint);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: env.mint.to_bytes(),
            secondary_mint: secondary_mint.to_bytes(),
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(secondary_mint, false),
        ],
        &[&admin],
    )
    .expect("configure market B secondary mint");
    env.svm
        .set_account(
            secondary_vault_b,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, vault_authority_b, 70),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    env.resolve();
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);
    let primary_dest = env.token_account(admin.pubkey(), 0);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let primary_vault_b_before = env.svm.get_account(&primary_vault_b).unwrap();
    let secondary_vault_a_before = env.svm.get_account(&secondary_vault_a).unwrap();
    let secondary_vault_b_before = env.svm.get_account(&secondary_vault_b).unwrap();
    let primary_dest_before = env.svm.get_account(&primary_dest).unwrap();
    let secondary_dest_before = env.svm.get_account(&secondary_dest).unwrap();

    let close_with_vaults = |env: &mut V16CuEnv,
                             primary_vault: Pubkey,
                             secondary_vault: Pubkey|
     -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(primary_vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(primary_dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new(secondary_dest, false),
            ],
            &[&admin],
        )
    };

    for (label, primary_vault, secondary_vault) in [
        (
            "foreign primary reserve",
            primary_vault_b,
            secondary_vault_a,
        ),
        ("foreign secondary reserve", env.vault, secondary_vault_b),
    ] {
        let rejected = close_with_vaults(&mut env, primary_vault, secondary_vault);
        assert!(rejected.is_err(), "CloseSlab must reject {label}");
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
        assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            primary_vault_before
        );
        assert_eq!(
            env.svm.get_account(&primary_vault_b).unwrap(),
            primary_vault_b_before
        );
        assert_eq!(
            env.svm.get_account(&secondary_vault_a).unwrap(),
            secondary_vault_a_before
        );
        assert_eq!(
            env.svm.get_account(&secondary_vault_b).unwrap(),
            secondary_vault_b_before
        );
        assert_eq!(
            env.svm.get_account(&primary_dest).unwrap(),
            primary_dest_before
        );
        assert_eq!(
            env.svm.get_account(&secondary_dest).unwrap(),
            secondary_dest_before
        );
    }

    let primary_vault_a = env.vault;
    let ok = close_with_vaults(&mut env, primary_vault_a, secondary_vault_a);
    assert!(
        ok.is_ok(),
        "same-market secondary reserve CloseSlab succeeds: {ok:?}"
    );
    assert_eq!(env.token_amount(primary_dest), 7);
    assert_eq!(env.token_amount(secondary_dest), 50);
    assert_eq!(env.token_amount(secondary_vault_b), 70);
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_eq!(closed_market.lamports, 0);
    assert!(closed_market.data.iter().all(|b| *b == 0));
}

// security.md sweep — ForceCloseAbandonedAsset has no portfolio-owner signatures by design after a
// shutdown timeout, so it must prove portfolio provenance itself. A cranker must not be able to pair a
// market-A portfolio with a market-B abandoned asset and mutate either market/account. Same-market
// control proves the rejected path is specifically the foreign portfolio binding.
#[test]
fn v16_attack_force_close_rejects_cross_market_portfolio_substitution() {
    const DELAY: u64 = 5;
    const SHUT: u64 = 20;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    env.configure_permissionless_resolve_with_cu(100, DELAY);

    let a_long_owner = Keypair::new();
    let a_short_owner = Keypair::new();
    let a_long = env.create_portfolio(&a_long_owner);
    let a_short = env.create_portfolio(&a_short_owner);
    env.deposit(&a_long_owner, a_long, 1_000_000);
    env.deposit(&a_short_owner, a_short, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &a_long_owner,
        a_long,
        &a_short_owner,
        a_short,
        POS_SCALE as i128,
        100,
        0,
    );
    assert_eq!(
        env.portfolio_state(a_long)
            .provenance_header
            .market_group_id,
        env.market.to_bytes(),
        "foreign candidate is genuinely bound to market A"
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(a_long), 1).side,
        SideV16::Long
    );

    let params = V16CuMarketParams {
        max_portfolio_assets: 2,
        ..V16CuMarketParams::default()
    };
    let (market_b, _vault_authority_b, vault_b) =
        init_independent_market_same_mint(&mut env, params);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 1,
            initial_mark_e6: 100,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
        ],
        &[&env.admin],
    )
    .expect("configure market B asset-1 auth mark");
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 100,
            force_close_delay_slots: DELAY,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
        ],
        &[&env.admin],
    )
    .expect("configure market B force-close delay");

    let b_long_owner = Keypair::new();
    let b_short_owner = Keypair::new();
    let b_long = init_portfolio_on_market(
        &mut env,
        market_b,
        &b_long_owner,
        params.max_portfolio_assets as usize,
    );
    let b_short = init_portfolio_on_market(
        &mut env,
        market_b,
        &b_short_owner,
        params.max_portfolio_assets as usize,
    );
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &b_long_owner,
        b_long,
        1_000_000,
    );
    deposit_to_market(
        &mut env,
        market_b,
        vault_b,
        &b_short_owner,
        b_short,
        1_000_000,
    );
    env.svm.expire_blockhash();
    let b_long_id = env.portfolio_id(b_long);
    let b_short_id = env.portfolio_id(b_short);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: b_long_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: b_short_id,
            account_b_position_epoch: 0,
            asset_index: 1,
            market_id: first_generation_market_id((1) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        vec![
            AccountMeta::new(b_long_owner.pubkey(), true),
            AccountMeta::new(b_short_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(b_long, false),
            AccountMeta::new(b_short, false),
        ],
        &[&b_long_owner, &b_short_owner],
    )
    .expect("open market B asset-1 position");
    let (_, market_b_open) =
        state::read_market(&env.svm.get_account(&market_b).unwrap().data).expect("read market B");
    assert_eq!(market_b_open.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(market_b_open.assets[1].oi_eff_short_q, POS_SCALE);

    env.svm.warp_to_slot(SHUT);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            asset_index: 1,
            market_id: market_b_open.assets[1].market_id,
            now_slot: SHUT,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: env.admin.pubkey().to_bytes(),
            insurance_operator: env.admin.pubkey().to_bytes(),
            backing_bucket_authority: env.admin.pubkey().to_bytes(),
            oracle_authority: env.admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
        ],
        &[&env.admin],
    )
    .expect("shut down market B asset 1");
    env.svm.warp_to_slot(SHUT + DELAY + 1);

    let cranker = Keypair::new();
    env.ensure_signer_account(cranker.pubkey());
    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let a_long_before = env.svm.get_account(&a_long).unwrap();
    let b_short_before = env.svm.get_account(&b_short).unwrap();
    let vault_b_before = env.svm.get_account(&vault_b).unwrap();
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ForceCloseAbandonedAsset {
            asset_index: 1,
            now_slot: SHUT + DELAY + 1,
            close_q: POS_SCALE,
        },
        vec![
            AccountMeta::new(cranker.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(a_long, false),
            AccountMeta::new(b_short, false),
        ],
        &[&cranker],
    );
    assert!(
        rejected.is_err(),
        "permissionless force-close must reject a market-A portfolio under market B"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_a_before);
    assert_eq!(env.svm.get_account(&market_b).unwrap(), market_b_before);
    assert_eq!(env.svm.get_account(&a_long).unwrap(), a_long_before);
    assert_eq!(env.svm.get_account(&b_short).unwrap(), b_short_before);
    assert_eq!(env.svm.get_account(&vault_b).unwrap(), vault_b_before);

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ForceCloseAbandonedAsset {
            asset_index: 1,
            now_slot: SHUT + DELAY + 1,
            close_q: POS_SCALE,
        },
        vec![
            AccountMeta::new(cranker.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(b_long, false),
            AccountMeta::new(b_short, false),
        ],
        &[&cranker],
    );
    assert!(
        ok.is_ok(),
        "same-market abandoned pair force-closes: {ok:?}"
    );
    let (_, market_b_after) = state::read_market(&env.svm.get_account(&market_b).unwrap().data)
        .expect("read market B after force-close");
    assert_eq!(market_b_after.assets[1].oi_eff_long_q, 0);
    assert_eq!(market_b_after.assets[1].oi_eff_short_q, 0);
    assert_eq!(market_b_after.vault as u64, env.token_amount(vault_b));
    assert!(market_b_after.vault >= market_b_after.c_tot + market_b_after.insurance);
}

// security.md sweep — liquidation/ADL isolation across assets (#9/#22 interaction): a liquidation (and
// its ADL deleverage) on asset A's market must not touch asset B's independent book. Attacker goal: a
// liquidation on one asset corrupts another asset's OI / a-factor / mark, or socializes A's loss onto
// B's holders. Protection: OI, a_long/a_short and effective_price are per-asset; asset B is byte-stable.
#[test]
fn v16_attack_liquidation_isolated_across_assets() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    // asset 0: a0 long vs b0 short (b0 will be liquidated)
    let a0o = Keypair::new();
    let a0 = env.create_portfolio(&a0o);
    let b0o = Keypair::new();
    let b0 = env.create_portfolio(&b0o);
    // asset 1: a1 long vs b1 short (independent, must be untouched)
    let a1o = Keypair::new();
    let a1 = env.create_portfolio(&a1o);
    let b1o = Keypair::new();
    let b1 = env.create_portfolio(&b1o);
    for (o, p, amount) in [
        (&a0o, a0, 1_000),
        (&b0o, b0, 900),
        (&a1o, a1, 1_000),
        (&b1o, b1, 1_000),
    ] {
        env.deposit(o, p, amount);
    }
    env.trade_asset_with_cu(0, &a0o, a0, &b0o, b0, (2 * POS_SCALE) as i128, 100, 0);
    env.trade_asset_with_cu(1, &a1o, a1, &b1o, b1, (3 * POS_SCALE) as i128, 100, 0);

    // snapshot asset 1's book BEFORE touching asset 0.
    let g_pre = env.market_state().1;
    let b1_oi_long = g_pre.assets[1].oi_eff_long_q;
    let b1_oi_short = g_pre.assets[1].oi_eff_short_q;
    let b1_a_long = g_pre.assets[1].a_long;
    let b1_a_short = g_pre.assets[1].a_short;
    let b1_price = g_pre.assets[1].effective_price;
    let a1_pre = env.portfolio_state(a1);
    let b1_pre = env.portfolio_state(b1);
    assert_eq!(b1_oi_long, 3 * POS_SCALE, "asset 1 carries its own OI");

    // drive ONLY asset 0's mark up; settle asset-0 accounts; partial-liquidate b0 (ADL on asset 0).
    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 500);
    for p in [b0, a0] {
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
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
    env.crank_steps_after_market_catchup(
        b0,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
        2,
    );
    let g_post = env.market_state().1;
    assert!(
        g_post.assets[0].a_long < ADL_ONE,
        "ADL engaged on asset 0 (non-vacuous)"
    );

    // ISOLATION: asset 1's entire book is byte-identical — OI, a-factors, mark, and the asset-1 portfolios.
    assert_eq!(
        g_post.assets[1].oi_eff_long_q, b1_oi_long,
        "asset-1 long OI untouched by asset-0 liquidation"
    );
    assert_eq!(
        g_post.assets[1].oi_eff_short_q, b1_oi_short,
        "asset-1 short OI untouched"
    );
    assert_eq!(
        g_post.assets[1].a_long, b1_a_long,
        "asset-1 a_long untouched (no cross-asset ADL)"
    );
    assert_eq!(
        g_post.assets[1].a_short, b1_a_short,
        "asset-1 a_short untouched"
    );
    assert_eq!(
        g_post.assets[1].effective_price, b1_price,
        "asset-1 mark untouched"
    );
    assert_eq!(
        env.portfolio_state(a1).legs,
        a1_pre.legs,
        "asset-1 long portfolio legs untouched"
    );
    assert_eq!(
        env.portfolio_state(b1).legs,
        b1_pre.legs,
        "asset-1 short portfolio legs untouched"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation"
    );
}

// security.md sweep — liquidation cranker reward market binding (#3/#44): when cranker rewards are
// enabled, the reward portfolio must belong to the same market as the liquidated account. A foreign
// market portfolio can otherwise try to receive value from this market's insurance while validating under
// different market provenance. Rejection must be atomic; a same-market cranker remains the positive path.
#[test]
fn v16_attack_liquidation_rejects_cross_market_cranker_reward() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(s)).certified_liq_deficit != 0,
        "short is liquidatable before the reward-account substitution attempt"
    );

    let market_b = Pubkey::new_unique();
    env.svm
        .set_account(
            market_b,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let p = V16CuMarketParams::default();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: p.max_portfolio_assets,
            h_min: p.h_min,
            h_max: p.h_max,
            initial_price: p.initial_price,
            min_nonzero_mm_req: p.min_nonzero_mm_req,
            min_nonzero_im_req: p.min_nonzero_im_req,
            maintenance_margin_bps: p.maintenance_margin_bps,
            initial_margin_bps: p.initial_margin_bps,
            max_trading_fee_bps: p.max_trading_fee_bps,
            trade_fee_base_bps: p.trade_fee_base_bps,
            liquidation_fee_bps: p.liquidation_fee_bps,
            liquidation_fee_cap: p.liquidation_fee_cap,
            min_liquidation_abs: p.min_liquidation_abs,
            max_price_move_bps_per_slot: p.max_price_move_bps_per_slot,
            max_accrual_dt_slots: p.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: p.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: p.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: p.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: p.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: p.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: p.public_b_chunk_atoms,
            maintenance_fee_per_slot: p.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    )
    .expect("init market B");

    let foreign_owner = Keypair::new();
    env.ensure_signer_account(foreign_owner.pubkey());
    let foreign_cranker = Pubkey::new_unique();
    env.svm
        .set_account(
            foreign_cranker,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; env.portfolio_account_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(foreign_owner.pubkey(), true),
            AccountMeta::new(market_b, false),
            AccountMeta::new(foreign_cranker, false),
        ],
        &[&foreign_owner],
    )
    .expect("init foreign cranker portfolio on market B");

    let market_a_before = env.svm.get_account(&env.market).unwrap();
    let market_b_before = env.svm.get_account(&market_b).unwrap();
    let short_before = env.svm.get_account(&s).unwrap();
    let foreign_before = env.svm.get_account(&foreign_cranker).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(foreign_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(foreign_cranker, false),
        ],
        &[&foreign_owner],
    );
    assert!(
        rejected.is_err(),
        "foreign-market cranker reward portfolio must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_a_before,
        "cross-market reward rejection leaves market A byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&market_b).unwrap(),
        market_b_before,
        "cross-market reward rejection leaves market B byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&s).unwrap(),
        short_before,
        "cross-market reward rejection leaves the liquidated account byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&foreign_cranker).unwrap(),
        foreign_before,
        "cross-market reward rejection does not credit the foreign cranker"
    );

    let local_cranker = env.create_portfolio(&foreign_owner);
    let local_cap_before = env.portfolio_state(local_cranker).capital.get();
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(foreign_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(local_cranker, false),
        ],
        &[&foreign_owner],
    );
    assert!(
        accepted.is_ok(),
        "same-market cranker reward liquidation still succeeds: {:?}",
        accepted
    );
    assert!(
        env.portfolio_state(local_cranker).capital.get() > local_cap_before,
        "positive control: same-market cranker receives a real reward"
    );
    let (_, group) = env.market_state();
    assert_eq!(
        group.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        group.vault >= group.c_tot + group.insurance,
        "senior conservation"
    );
}

// security.md sweep — market capacity: >64 assets per market + a position holding any 14 legs (#22/#32).
// The real per-market asset count is config.max_market_slots (u32), grown one slot at a time by
// handle_update_asset_lifecycle's append/realloc path.
// There is NO hardcoded 64 cap. This test proves: (1) a single market grows to >64 assets, (2) the per-
// position leg cap stays at WRAPPER_MAX_PORTFOLIO_ASSETS=14 INDEPENDENT of the market's asset count, and
// (3) a position can carry 14 legs drawn from arbitrary HIGH indices across the full set (not just 0..13).
// security.md sweep — cross-asset domain-insurance isolation (#22/#32): a permissionless asset is
// untrusted, so a position/insolvency on asset 1 must NEVER consume asset 0's domain insurance. The
// existing isolation test (#13376) checks OI/ADL/price isolation; this checks the per-DOMAIN INSURANCE
// budget. Attacker goal: drive an insolvency (bad debt) on asset 1 and have its socialization reach into
// asset 0's funded domain-insurance budget (draining funds that back asset-0 traders). Protection: domain
// insurance budgets are per-domain (asset i -> domains 2i, 2i+1); asset-1 bad debt is bounded to asset-1's
// own domains, so asset 0's domain-insurance bytes are unchanged. Senior conservation holds throughout.
#[test]
fn v16_attack_asset1_insolvency_cannot_drain_asset0_domain_insurance() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    // fund BOTH assets' domain insurance (asset 0 -> domains 0,1; asset 1 -> domains 2,3). admin is the
    // domain insurance authority for every domain (set at init / activation).
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 0, 1_000);
    env.top_up_insurance_domain_with_authority(&admin, 1, 1_000);
    env.top_up_insurance_domain_with_authority(&admin, 2, 500);
    env.top_up_insurance_domain_with_authority(&admin, 3, 500);
    let g0 = env.market_state().1;
    let a0_dom0 = g0.insurance_domain_budget[0];
    let a0_dom1 = g0.insurance_domain_budget[1];
    assert_eq!(a0_dom0, 1_000, "asset-0 long-domain insurance funded");
    assert_eq!(a0_dom1, 1_000, "asset-0 short-domain insurance funded");

    // asset-1 position: tiny-capital short that will be driven insolvent (bad debt on asset 1).
    let a1o = Keypair::new();
    let a1 = env.create_portfolio(&a1o);
    let b1o = Keypair::new();
    let b1 = env.create_portfolio(&b1o);
    env.deposit(&a1o, a1, 1_000_000);
    env.deposit(&b1o, b1, 250); // tiny -> insolvent short
    env.trade_asset_with_cu(1, &a1o, a1, &b1o, b1, POS_SCALE as i128, 100, 0);

    // push ONLY asset-1 mark up across slots (each step <= 2x, within the 100%/slot breaker): 100->800.
    // short's loss (units * (P-100)) = 1 * 700 = 700 >> 250 capital -> insolvent.
    for (slot, mark) in [(2u64, 200u64), (3, 400), (4, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(1, slot, mark);
        for p in [a1, b1] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
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
    // liquidate the insolvent asset-1 short (socializes its bad debt).
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b1, false),
        ],
        &[],
    );

    let g1 = env.market_state().1;
    // non-vacuity: asset-1 actually reached insolvency.
    assert_eq!(
        env.portfolio_state(b1).capital.get(),
        0,
        "asset-1 short was driven insolvent (non-vacuous)"
    );
    assert!(
        g1.assets[1].effective_price >= 200,
        "asset-1 mark actually moved (non-vacuous, got {})",
        g1.assets[1].effective_price
    );
    // ISOLATION (headline): asset-0's domain insurance budgets are byte-identical — asset-1 bad debt
    // could not reach into asset-0's insurance.
    assert_eq!(
        g1.insurance_domain_budget[0], a0_dom0,
        "asset-0 long-domain insurance UNTOUCHED by asset-1 insolvency"
    );
    assert_eq!(
        g1.insurance_domain_budget[1], a0_dom1,
        "asset-0 short-domain insurance UNTOUCHED by asset-1 insolvency"
    );
    // senior conservation + accounting integrity across the whole sequence.
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation under cross-asset insolvency"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
}

// Product spec — cross-asset BACKING isolation (counterpart to the domain-insurance isolation test):
// a faulty/insolvent permissionless asset must not drain ANOTHER asset's backing bucket. Fund both
// assets' backing, drive asset 1 insolvent + liquidate, and assert asset 0's backing buckets are
// byte-identical.
#[test]
fn v16_attack_asset1_insolvency_cannot_drain_asset0_backing() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    // fund BOTH assets' backing buckets (asset 0 -> domains 0,1; asset 1 -> domains 2,3), long expiry.
    env.top_up_backing_bucket(0, 1_000, 100_000);
    env.top_up_backing_bucket(1, 1_000, 100_000);
    env.top_up_backing_bucket(2, 500, 100_000);
    env.top_up_backing_bucket(3, 500, 100_000);
    let bk = |env: &V16CuEnv, d: usize| {
        env.market_state().1.source_backing_buckets[d].fresh_unliened_backing_num
    };
    let (a0d0, a0d1) = (bk(&env, 0), bk(&env, 1));
    assert!(a0d0 > 0 && a0d1 > 0, "asset-0 backing funded (non-vacuous)");

    // asset-1: tiny-capital short driven insolvent (bad debt on asset 1).
    let a1o = Keypair::new();
    let a1 = env.create_portfolio(&a1o);
    let b1o = Keypair::new();
    let b1 = env.create_portfolio(&b1o);
    env.deposit(&a1o, a1, 1_000_000);
    env.deposit(&b1o, b1, 250);
    env.trade_asset_with_cu(1, &a1o, a1, &b1o, b1, POS_SCALE as i128, 100, 0);
    for (slot, mark) in [(2u64, 200u64), (3, 400), (4, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(1, slot, mark);
        for p in [a1, b1] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
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
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b1, false),
        ],
        &[],
    );

    let g1 = env.market_state().1;
    assert_eq!(
        env.portfolio_state(b1).capital.get(),
        0,
        "asset-1 short driven insolvent (non-vacuous)"
    );
    // ISOLATION: asset-0's backing buckets are byte-identical — asset-1 bad debt can't reach asset-0 backing.
    assert_eq!(
        g1.source_backing_buckets[0].fresh_unliened_backing_num, a0d0,
        "asset-0 long-domain backing UNTOUCHED"
    );
    assert_eq!(
        g1.source_backing_buckets[1].fresh_unliened_backing_num, a0d1,
        "asset-0 short-domain backing UNTOUCHED"
    );
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation under cross-asset insolvency"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
}

#[test]
fn v16_bpf_permissionless_asset_cannot_withdraw_unrelated_domain_insurance() {
    let mut env = V16CuEnv::new();
    let victim_insurance = Keypair::new();

    env.activate_asset_with_authorities(
        1,
        1,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.activate_asset_with_authorities(
        2,
        2,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.top_up_insurance(500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 2, 500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 4, 500);

    let before_vault = env.token_amount(env.vault);
    let (_, before_group) = env.market_state();
    assert_eq!(before_vault, 1_500);
    assert_eq!(before_group.insurance, 1_500);
    assert_eq!(before_group.insurance_domain_budget[0], 250);
    assert_eq!(before_group.insurance_domain_budget[1], 250);
    assert_eq!(before_group.insurance_domain_budget[2], 500);
    assert_eq!(before_group.insurance_domain_budget[4], 500);

    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(3);
    let (_fee_source, _cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        3,
        3,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    let after_create_vault = env.token_amount(env.vault);
    let (_, after_create_group) = env.market_state();
    assert_eq!(
        after_create_group.assets[3].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(
        after_create_group.insurance_domain_budget[6], 0,
        "new attacker-controlled domain must not inherit shared insurance"
    );
    assert_eq!(
        after_create_group.insurance_domain_budget[7], 0,
        "new attacker-controlled domain must not inherit shared insurance"
    );
    assert_eq!(
        after_create_vault, 1_501,
        "only the permissionless init fee should enter the shared vault"
    );

    assert!(
        env.try_withdraw_insurance_domain_with_authority(&attacker, 6, 840)
            .is_err(),
        "attacker must not withdraw victim-funded insurance through domain 6"
    );
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&attacker, 7, 660)
            .is_err(),
        "attacker must not withdraw victim-funded insurance through domain 7"
    );

    let (_, final_group) = env.market_state();
    assert_eq!(env.token_amount(env.vault), after_create_vault);
    assert_eq!(final_group.insurance, 1_501);
    assert_eq!(final_group.vault, 1_501);
    assert_eq!(final_group.insurance_domain_budget[0], 250);
    assert_eq!(final_group.insurance_domain_budget[1], 251);
    assert_eq!(final_group.insurance_domain_budget[2], 500);
    assert_eq!(final_group.insurance_domain_budget[4], 500);
    assert_eq!(final_group.insurance_domain_budget[6], 0);
    assert_eq!(final_group.insurance_domain_budget[7], 0);
}

#[test]
fn v16_bpf_permissionless_oracle_liquidation_uses_only_its_own_domain_insurance() {
    let mut env = V16CuEnv::new();
    let victim_insurance = Keypair::new();
    let attacker = Keypair::new();

    env.activate_asset_with_authorities(
        1,
        1,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.activate_asset_with_authorities(
        2,
        2,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.top_up_insurance(500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 2, 500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 4, 500);

    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(3);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        3,
        3,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );
    env.svm.warp_to_slot(4);
    env.configure_auth_mark_for_asset_with_authority(3, &attacker, 4, 100);
    env.top_up_insurance_domain_with_authority(&attacker, 6, 300);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 200);
    env.trade_asset_with_cu(
        3,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(5);
    env.push_auth_mark_for_asset_with_authority(3, &attacker, 5, 1_000);
    for now_slot in [5u64, 6] {
        env.svm.warp_to_slot(now_slot);
        env.crank(
            long_account,
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: crank_observations(3),
            },
        );
    }
    let (_, before_liq) = env.market_state();
    assert_eq!(before_liq.insurance_domain_budget[0], 250);
    assert_eq!(before_liq.insurance_domain_budget[1], 251);
    assert_eq!(before_liq.insurance_domain_budget[2], 500);
    assert_eq!(before_liq.insurance_domain_budget[4], 500);
    assert_eq!(before_liq.insurance_domain_budget[6], 300);
    assert_eq!(before_liq.insurance_domain_spent[6], 0);
    assert_eq!(before_liq.insurance, 1_801);

    env.svm.warp_to_slot(7);
    let liq_cu = env.crank_steps(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 7,
            observations: crank_observations(3),
        },
        2,
    );
    println!("v16 permissionless malicious-oracle liquidation CU: {liq_cu}");

    let (_, after_liq) = env.market_state();
    let own_domain_spent = after_liq.insurance_domain_spent[6];
    assert!(
        own_domain_spent > 0,
        "malicious asset liquidation should consume its own funded domain"
    );
    assert_eq!(
        before_liq.insurance - after_liq.insurance,
        own_domain_spent,
        "aggregate insurance decrease must be exactly the attacker-domain spend"
    );
    assert_eq!(after_liq.insurance_domain_budget[0], 250);
    assert_eq!(after_liq.insurance_domain_budget[1], 251);
    assert_eq!(after_liq.insurance_domain_budget[2], 500);
    assert_eq!(after_liq.insurance_domain_budget[4], 500);
    assert_eq!(after_liq.insurance_domain_spent[0], 0);
    assert_eq!(after_liq.insurance_domain_spent[1], 0);
    assert_eq!(after_liq.insurance_domain_spent[2], 0);
    assert_eq!(after_liq.insurance_domain_spent[4], 0);
}
