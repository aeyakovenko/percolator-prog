//! INV-033 - Insurance-backed lien single classification.
//!
//! Normative obligation: an insurance-backed source-credit lien, if exposed by a
//! public route, must be classified exactly once as insurance backing and never
//! also as counterparty backing or generic support.
//!
//! Current wrapper boundary: public domain-insurance top-up funds insurance
//! budgets, but it does not expose the engine's insurance-credit reservation
//! primitive. This test therefore locks down the deployed public behavior:
//! ordinary risk-increase may create a counterparty-backed source lien when a
//! fresh backing bucket exists, but the same route must not silently consume
//! unreserved domain insurance or populate the insurance-backed lien fields.
//! The mixed-reserve bundle adds a live-lien payout boundary: insurance cannot
//! replace backing already reserved for risk, even within an atomic transaction.

use super::*;

#[test]
fn v16_program_mixed_reserve_payout_bundle_preserves_live_lien_classification() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const DOMAIN: usize = 1;
    const DEPOSIT: u128 = 313;
    const COUNTERPARTY_DEPOSIT: u128 = 1_000;
    const BACKING: u128 = 150;
    const INSURANCE: u128 = 83;
    const CLAIM: u128 = 20 * (105 - 100);
    const LOSS: u128 = 10 * (100 - 95);
    // Per-leg initial margin rounds up: 210 + ceil(11 * 95 / 10) = 315.
    const LIEN: u128 = 210 + (11 * 95 + 9) / 10 - (DEPOSIT - LOSS);
    // The counterparty's settled 100-atom loss also enters this backing bucket.
    const TOTAL_BACKING: u128 = BACKING + CLAIM;
    // Liened backing cannot also satisfy the remaining generic source-credit cap.
    const SURPLUS: u128 = TOTAL_BACKING - CLAIM - LIEN;
    const SUPPLY: u128 = DEPOSIT + COUNTERPARTY_DEPOSIT + BACKING + INSURANCE;

    let mut env = inv018_public_spl_market_with_params(
        6,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    let owners = [
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
        env.admin.insecure_clone(),
    ];
    for owner in &owners[..3] {
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    }
    for role in [
        processor::ASSET_AUTH_INSURANCE,
        processor::ASSET_AUTH_INSURANCE_OPERATOR,
    ] {
        env.try_update_per_asset_authority_with_cu(
            &owners[3],
            Some(&owners[2]),
            0,
            role,
            owners[2].pubkey().to_bytes(),
        )
        .unwrap();
    }
    let wallets = owners.each_ref().map(Signer::pubkey);
    let tokens =
        wallets.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
    for (token, amount) in
        tokens
            .into_iter()
            .zip([DEPOSIT, COUNTERPARTY_DEPOSIT, INSURANCE, BACKING])
    {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &wallets[3],
                &[],
                amount as u64,
            )
            .unwrap(),
            &[&owners[3]],
        )
        .unwrap();
    }
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &wallets[3],
            &[],
        )
        .unwrap(),
        &[&owners[3]],
    )
    .unwrap();
    let mint_before = env.svm.get_account(&env.mint).unwrap();
    let mint = Mint::unpack(&mint_before.data).unwrap();
    assert_eq!(
        (u128::from(mint.supply), mint.mint_authority),
        (SUPPLY, COption::None)
    );

    let funding_accounts = |env: &V16CuEnv, actor: usize| {
        vec![
            AccountMeta::new(wallets[actor], true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(tokens[actor], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ]
    };
    let mut portfolios = Vec::new();
    for actor in 0..2 {
        let key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            env.portfolio_account_len,
            env.program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(wallets[actor], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key.pubkey(), false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
        let mut accounts = funding_accounts(&env, actor);
        accounts.insert(2, AccountMeta::new(key.pubkey(), false));
        env.send(
            env.deposit_ix(key.pubkey(), [DEPOSIT, COUNTERPARTY_DEPOSIT][actor]),
            accounts,
            &[&owners[actor]],
        )
        .unwrap();
        portfolios.push(key.pubkey());
        env.portfolios.push(key.pubkey());
    }
    let [winner, counterparty] = [portfolios[0], portfolios[1]];
    let epoch = env.control_sequences(0).authority_epoch;
    for (actor, ix) in [
        (
            2,
            ProgInstruction::TopUpInsuranceDomain {
                domain: DOMAIN as u16,
                market_id: env.asset_market_id(0),
                authority_epoch: epoch,
                intent_id: 0,
                amount: INSURANCE,
            },
        ),
        (
            3,
            ProgInstruction::TopUpBackingBucket {
                domain: DOMAIN as u16,
                market_id: env.asset_market_id(0),
                authority_epoch: epoch,
                intent_id: 0,
                backing_fee_bps: 0,
                insurance_share_bps: 0,
                amount: BACKING,
                expiry_slot: 100,
            },
        ),
    ] {
        let cu = env
            .send(ix, funding_accounts(&env, actor), &[&owners[actor]])
            .unwrap();
        assert_cu_within("INV-033 mixed reserve funding", cu, CUSTODY_CU_LIMIT);
    }

    env.svm.warp_to_slot(1);
    for asset in 0..2 {
        env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        let cu = env.trade_asset_with_cu(
            asset,
            &owners[0],
            winner,
            &owners[1],
            counterparty,
            [20, 10][asset as usize] * POS_SCALE as i128,
            100,
            0,
        );
        assert_cu_within("INV-033 open", cu, MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
    }
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset) in [(counterparty, 0), (winner, 0), (counterparty, 1)] {
        let cu = env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset, 1 - asset]),
            },
        );
        assert_cu_within("INV-033 settle", cu, CRANK_CU_LIMIT);
    }
    let trade_cu = env.trade_asset_with_cu(
        1,
        &owners[0],
        winner,
        &owners[1],
        counterparty,
        POS_SCALE as i128,
        95,
        0,
    );
    assert_cu_within(
        "INV-033 create mixed-funded lien",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let assert_state = |env: &V16CuEnv,
                        paid_insurance: u128,
                        paid_backing: u128,
                        lien: u128,
                        converted: bool,
                        withdrawn: bool| {
        let group = env.market_state().1;
        let a = env.portfolio_state(winner);
        let b = env.portfolio_state(counterparty);
        let source = state::portfolio_source_domain(&a, DOMAIN);
        let remaining_claim = if converted { 0 } else { CLAIM };
        let consumed = if converted { CLAIM } else { 0 };
        let payout = if withdrawn { DEPOSIT - LOSS + CLAIM } else { 0 };
        assert_eq!(
            (a.capital.get(), a.pnl.get()),
            (DEPOSIT - LOSS + consumed - payout, remaining_claim as i128)
        );
        assert_eq!(
            (b.capital.get(), b.pnl.get()),
            (COUNTERPARTY_DEPOSIT - CLAIM, LOSS as i128)
        );
        assert_eq!(
            source.source_claim_bound_num.get(),
            remaining_claim * BOUND_SCALE
        );
        assert_eq!(source.source_claim_liened_num.get(), lien * BOUND_SCALE);
        assert_eq!(
            source.source_claim_counterparty_liened_num.get(),
            lien * BOUND_SCALE
        );
        assert_eq!(
            source.source_lien_counterparty_backing_num.get(),
            lien * BOUND_SCALE
        );
        assert_eq!(source.source_lien_effective_reserved.get(), lien);
        let credit = &group.source_credit[DOMAIN];
        let bucket = &group.source_backing_buckets[DOMAIN];
        let backing = TOTAL_BACKING - paid_backing - consumed;
        assert_eq!(
            credit.positive_claim_bound_num,
            remaining_claim * BOUND_SCALE
        );
        assert_eq!(
            credit.exact_positive_claim_num,
            remaining_claim * BOUND_SCALE
        );
        assert_eq!(credit.fresh_reserved_backing_num, backing * BOUND_SCALE);
        assert_eq!(credit.valid_liened_backing_num, lien * BOUND_SCALE);
        assert_eq!(credit.spent_backing_num, consumed * BOUND_SCALE);
        assert_eq!(credit.provider_receivable_num, consumed * BOUND_SCALE);
        assert_eq!(credit.credit_rate_num, percolator::CREDIT_RATE_SCALE);
        assert_eq!(
            bucket.fresh_unliened_backing_num,
            (backing - lien) * BOUND_SCALE
        );
        assert_eq!(bucket.valid_liened_backing_num, lien * BOUND_SCALE);
        assert_eq!(bucket.consumed_liened_backing_num, consumed * BOUND_SCALE);
        for (account, face, reserved) in [(&a, remaining_claim, lien), (&b, LOSS, 0)] {
            assert_eq!(
                account
                    .source_domains
                    .iter()
                    .map(|slot| slot.source_claim_bound_num.get())
                    .sum::<u128>(),
                face * BOUND_SCALE
            );
            assert_eq!(
                account
                    .source_domains
                    .iter()
                    .map(|slot| slot.source_claim_liened_num.get())
                    .sum::<u128>(),
                reserved * BOUND_SCALE
            );
            assert_eq!(
                account
                    .source_domains
                    .iter()
                    .map(|slot| slot.source_lien_counterparty_backing_num.get())
                    .sum::<u128>(),
                reserved * BOUND_SCALE
            );
            for slot in &account.source_domains {
                assert_eq!(slot.source_claim_insurance_liened_num.get(), 0);
                assert_eq!(slot.source_lien_insurance_backing_num.get(), 0);
                assert_eq!(slot.source_lien_capital_at_risk_fee_revenue.get(), 0);
            }
        }
        for (domain, credit) in group.source_credit.iter().enumerate() {
            assert_eq!(credit.insurance_credit_reserved_num, 0);
            assert_eq!(credit.valid_liened_insurance_num, 0);
            assert_eq!(credit.impaired_liened_insurance_num, 0);
            assert_eq!(credit.impaired_liened_backing_num, 0);
            assert_eq!(
                group.source_backing_buckets[domain].impaired_liened_backing_num,
                0
            );
            assert_eq!(
                group.source_backing_buckets[domain].utilization_fee_earnings,
                0
            );
            assert_eq!(group.insurance_domain_spent[domain], 0);
            assert_eq!(
                group.insurance_domain_budget[domain],
                if domain == DOMAIN {
                    INSURANCE - paid_insurance
                } else {
                    0
                }
            );
            if domain != DOMAIN {
                let peer_claim = if domain == 2 { LOSS } else { 0 };
                assert_eq!(credit.positive_claim_bound_num, peer_claim * BOUND_SCALE);
                assert_eq!(credit.exact_positive_claim_num, peer_claim * BOUND_SCALE);
                assert_eq!(credit.fresh_reserved_backing_num, peer_claim * BOUND_SCALE);
                assert_eq!(credit.valid_liened_backing_num, 0);
                let peer_bucket = &group.source_backing_buckets[domain];
                assert_eq!(
                    peer_bucket.fresh_unliened_backing_num,
                    peer_claim * BOUND_SCALE
                );
                assert_eq!(peer_bucket.valid_liened_backing_num, 0);
                assert_eq!(peer_bucket.consumed_liened_backing_num, 0);
            }
        }
        assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        assert_eq!(
            group.source_claim_bound_total_num,
            (remaining_claim + LOSS) * BOUND_SCALE
        );
        assert_eq!(group.pnl_pos_tot, remaining_claim + LOSS);
        assert_eq!(group.insurance, INSURANCE - paid_insurance);
        assert_domain_budget_remaining_total_consistent(&group, "INV-033 disjoint insurance");
        assert_eq!(group.c_tot, a.capital.get() + b.capital.get());
        assert_eq!(group.vault, SUPPLY - paid_insurance - paid_backing - payout);
        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
        assert_eq!(group.vault, group.c_tot + group.insurance + backing + LOSS);
        assert_eq!(
            tokens.map(|key| u128::from(env.token_amount(key))),
            [payout, 0, paid_insurance, paid_backing]
        );
        assert_eq!(
            group.vault
                + tokens
                    .iter()
                    .map(|key| u128::from(env.token_amount(*key)))
                    .sum::<u128>(),
            SUPPLY
        );
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
    };
    assert_state(&env, 0, 0, LIEN, false, false);
    let portfolio_before = [winner, counterparty].map(|key| env.svm.get_account(&key));
    let payout_accounts = |env: &V16CuEnv, actor: usize| {
        vec![
            AccountMeta::new(wallets[actor], true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(tokens[actor], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ]
    };
    let mut max_bundle_cu = 0;
    for (amount, accepted) in [(SURPLUS + 1, false), (SURPLUS, true)] {
        env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit((2 * CUSTODY_CU_LIMIT) as u32),
                Instruction {
                    program_id: env.program_id,
                    accounts: payout_accounts(&env, 2),
                    data: env
                        .withdraw_insurance_asset_instruction(wallets[2], 0, INSURANCE)
                        .encode(),
                },
                Instruction {
                    program_id: env.program_id,
                    accounts: payout_accounts(&env, 3),
                    data: ProgInstruction::WithdrawBackingBucket {
                        domain: DOMAIN as u16,
                        market_id: env.asset_market_id(0),
                        authority_epoch: epoch,
                        amount,
                    }
                    .encode(),
                },
            ],
            Some(&env.payer.pubkey()),
            &[&env.payer, &owners[2], &owners[3]],
            env.svm.latest_blockhash(),
        );
        let fee = u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let mut keys = tx.message.account_keys.clone();
        keys.extend([
            winner,
            counterparty,
            env.mint,
            tokens[0],
            tokens[1],
            wallets[0],
            wallets[1],
        ]);
        keys.sort();
        keys.dedup();
        let before = keys
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        let result = env.svm.send_transaction(tx);
        let meta = if accepted {
            result.expect("insurance and genuinely surplus backing are independently payable")
        } else {
            let failed = result.expect_err("insurance cannot replace one missing backing atom");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(
                    3,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                )
            );
            assert!(
                failed
                    .meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", spl_token::ID)),
                "insurance SPL transfer must complete before the backing rejection"
            );
            for (key, mut account) in keys.iter().zip(before) {
                if *key == env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    env.svm.get_account(key),
                    account,
                    "mixed-payout rollback: {key}"
                );
            }
            assert_state(&env, 0, 0, LIEN, false, false);
            failed.meta
        };
        assert_cu_within(
            "INV-033 mixed reserve payout bundle",
            meta.compute_units_consumed,
            2 * CUSTODY_CU_LIMIT,
        );
        max_bundle_cu = max_bundle_cu.max(meta.compute_units_consumed);
        assert_eq!(
            [winner, counterparty].map(|key| env.svm.get_account(&key)),
            portfolio_before
        );
    }
    assert_state(&env, INSURANCE, SURPLUS, LIEN, false, false);

    for (asset, size, price) in [(1, 11, 95), (0, 20, 105)] {
        let cu = env.trade_asset_with_cu(
            asset,
            &owners[0],
            winner,
            &owners[1],
            counterparty,
            -size * POS_SCALE as i128,
            price,
            0,
        );
        assert_cu_within(
            "INV-033 flatten after reserve payout",
            cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
        assert_state(&env, INSURANCE, SURPLUS, LIEN, false, false);
    }
    let flat_counterparty = env.svm.get_account(&counterparty);
    let mut release_cu = 0;
    let mut release_steps = 0;
    for _ in 0..8 {
        let a = env.portfolio_state(winner);
        if state::portfolio_source_domain(&a, DOMAIN)
            .source_lien_counterparty_backing_num
            .get()
            == 0
        {
            break;
        }
        let cu = env.crank(
            winner,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[0, 1]),
            },
        );
        assert_cu_within("INV-033 release", cu, CRANK_CU_LIMIT);
        release_cu = release_cu.max(cu);
        release_steps += 1;
        let a = env.portfolio_state(winner);
        let remaining = state::portfolio_source_domain(&a, DOMAIN)
            .source_lien_counterparty_backing_num
            .get();
        assert!(
            remaining == 0 || remaining == LIEN * BOUND_SCALE,
            "one-domain release cannot partially relabel the lien"
        );
        assert_state(
            &env,
            INSURANCE,
            SURPLUS,
            remaining / BOUND_SCALE,
            false,
            false,
        );
        assert_eq!(env.svm.get_account(&counterparty), flat_counterparty);
    }
    assert_state(&env, INSURANCE, SURPLUS, 0, false, false);
    assert!(release_cu > 0, "suffix must publicly release a real lien");
    for portfolio in [winner, counterparty] {
        assert!(percolator::active_bitmap_is_empty(active_bitmap(
            &env.portfolio_state(portfolio)
        )));
    }
    let released_payout_cu =
        env.withdraw_backing_bucket_to_admin_token_with_cu(tokens[3], DOMAIN as u16, LIEN);
    assert_cu_within(
        "INV-033 released backing payout",
        released_payout_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_state(&env, INSURANCE, BACKING, 0, false, false);
    // Provider withdrawal changes the risk epoch even for this now-flat owner.
    let refresh_cu = env.crank(
        winner,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_for_assets(&[0, 1]),
        },
    );
    assert_cu_within(
        "INV-033 post-payout recertification",
        refresh_cu,
        CRANK_CU_LIMIT,
    );
    assert_state(&env, INSURANCE, BACKING, 0, false, false);
    let convert_cu = env.convert_released_pnl_with_cu(&owners[0], winner, CLAIM);
    assert_cu_within("INV-033 consume backing once", convert_cu, CUSTODY_CU_LIMIT);
    assert_state(&env, INSURANCE, BACKING, 0, true, false);
    let cu = env
        .send(
            env.withdraw_ix(winner, DEPOSIT - LOSS + CLAIM),
            vec![
                AccountMeta::new(wallets[0], true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(winner, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[0]],
        )
        .unwrap();
    assert_cu_within("INV-033 owner payout", cu, CUSTODY_CU_LIMIT);
    assert_state(&env, INSURANCE, BACKING, 0, true, true);
    assert_eq!(env.svm.get_account(&counterparty), flat_counterparty);
    println!("INV-031/032/033 mixed reserve bundle: lien={LIEN}, insurance={INSURANCE}, provider={SURPLUS}+{LIEN}, claim={CLAIM}, owner={}, release_steps={release_steps}, CU trade={trade_cu}, bundle={max_bundle_cu}, release={release_cu}, released_payout={released_payout_cu}, refresh={refresh_cu}, conversion={convert_cu}, withdrawal={cu}", DEPOSIT - LOSS + CLAIM);
}

#[derive(Debug)]
struct SourceLienClassification {
    trade_succeeded: bool,
    source_claim_counterparty_liened_num: u128,
    source_claim_insurance_liened_num: u128,
    source_lien_counterparty_backing_num: u128,
    source_lien_insurance_backing_num: u128,
    market_valid_liened_backing_num: u128,
    market_insurance_credit_reserved_num: u128,
    market_valid_liened_insurance_num: u128,
    domain_insurance_budget: u128,
}

fn run_public_source_lien_classification(
    use_counterparty_backing: bool,
) -> SourceLienClassification {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 95;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const DEPOSIT: u128 = 313;
    const SOURCE_DOMAIN: usize = 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);

    if use_counterparty_backing {
        env.top_up_backing_bucket(SOURCE_DOMAIN as u16, 150, 10);
    } else {
        let admin = env.admin.insecure_clone();
        env.top_up_insurance_domain_with_authority(&admin, SOURCE_DOMAIN as u16, 150);
    }

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_cross = env.svm.get_account(&cross_account).unwrap();
    let before_counterparty = env.svm.get_account(&counterparty_account).unwrap();
    let before_vault = env.svm.get_account(&env.vault).unwrap();
    let trade_result = env.try_trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        SAFE_INCREASE_Q,
        ASSET1_MARK,
        0,
    );

    if use_counterparty_backing {
        assert!(
            trade_result.is_ok(),
            "fresh counterparty backing should admit the source-credit risk increase: {trade_result:?}",
        );
    } else {
        assert!(
            trade_result.is_err(),
            "unreserved domain insurance must not be silently consumed as source-credit backing",
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before_market,
            "rejected insurance-only source-credit attempt rewrote market state",
        );
        assert_eq!(
            env.svm.get_account(&cross_account).unwrap(),
            before_cross,
            "rejected insurance-only source-credit attempt rewrote the trader portfolio",
        );
        assert_eq!(
            env.svm.get_account(&counterparty_account).unwrap(),
            before_counterparty,
            "rejected insurance-only source-credit attempt rewrote the counterparty portfolio",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            before_vault,
            "rejected insurance-only source-credit attempt moved vault tokens",
        );
    }

    let cross_after = env.portfolio_state(cross_account);
    let source = state::portfolio_source_domain(&cross_after, SOURCE_DOMAIN);
    let (_, group_after) = env.market_state();
    SourceLienClassification {
        trade_succeeded: trade_result.is_ok(),
        source_claim_counterparty_liened_num: source.source_claim_counterparty_liened_num.get(),
        source_claim_insurance_liened_num: source.source_claim_insurance_liened_num.get(),
        source_lien_counterparty_backing_num: source.source_lien_counterparty_backing_num.get(),
        source_lien_insurance_backing_num: source.source_lien_insurance_backing_num.get(),
        market_valid_liened_backing_num: group_after.source_credit[SOURCE_DOMAIN]
            .valid_liened_backing_num,
        market_insurance_credit_reserved_num: group_after.source_credit[SOURCE_DOMAIN]
            .insurance_credit_reserved_num,
        market_valid_liened_insurance_num: group_after.source_credit[SOURCE_DOMAIN]
            .valid_liened_insurance_num,
        domain_insurance_budget: group_after.insurance_domain_budget[SOURCE_DOMAIN],
    }
}

#[test]
fn v16_program_public_source_lien_classification_never_double_counts_insurance() {
    let counterparty = run_public_source_lien_classification(true);
    assert!(counterparty.trade_succeeded);
    assert!(
        counterparty.source_claim_counterparty_liened_num > 0,
        "control route must create a real counterparty-backed claim lien",
    );
    assert!(
        counterparty.source_lien_counterparty_backing_num > 0,
        "control route must reserve real counterparty backing",
    );
    assert_eq!(
        counterparty.source_claim_insurance_liened_num, 0,
        "counterparty-backed route must not also classify the same claim as insurance-backed",
    );
    assert_eq!(
        counterparty.source_lien_insurance_backing_num, 0,
        "counterparty-backed route must not reserve insurance backing",
    );
    assert!(counterparty.market_valid_liened_backing_num > 0);
    assert_eq!(counterparty.market_insurance_credit_reserved_num, 0);
    assert_eq!(counterparty.market_valid_liened_insurance_num, 0);

    let insurance_only = run_public_source_lien_classification(false);
    assert!(!insurance_only.trade_succeeded);
    assert!(
        insurance_only.domain_insurance_budget > 0,
        "negative route must be nonvacuous: domain insurance was actually funded",
    );
    assert_eq!(
        insurance_only.source_claim_counterparty_liened_num, 0,
        "rejected insurance-only route must not create counterparty claim liens",
    );
    assert_eq!(
        insurance_only.source_claim_insurance_liened_num, 0,
        "unreserved domain insurance must not become an account-local insurance claim lien",
    );
    assert_eq!(
        insurance_only.source_lien_counterparty_backing_num, 0,
        "rejected insurance-only route must not reserve counterparty backing",
    );
    assert_eq!(
        insurance_only.source_lien_insurance_backing_num, 0,
        "unreserved domain insurance must not become account-local insurance backing",
    );
    assert_eq!(insurance_only.market_valid_liened_backing_num, 0);
    assert_eq!(
        insurance_only.market_insurance_credit_reserved_num, 0,
        "public domain-insurance top-up must not expose the engine-only insurance-credit reservation",
    );
    assert_eq!(
        insurance_only.market_valid_liened_insurance_num, 0,
        "rejected route must not consume reserved insurance",
    );

    // This is an intentional API absence, not an untested public transition. The wrapper may
    // serialize the engine-owned reservation fields, but it cannot create or mutate them. A new
    // callsite makes the insurance-lien lifecycle publicly reachable and must reopen INV-033.
    let wrapper = include_str!("../../../src/v16_program.rs");
    for engine_method in [
        "reserve_insurance_credit_not_atomic(",
        "create_source_credit_lien_from_insurance_not_atomic(",
        "release_source_credit_lien_from_insurance_not_atomic(",
        "consume_source_credit_lien_from_insurance_not_atomic(",
        "impair_source_credit_lien_from_insurance_not_atomic(",
    ] {
        assert_eq!(
            wrapper.matches(engine_method).count(),
            0,
            "wrapper exposed engine-only insurance-lien method {engine_method}",
        );
    }

    crate::assert_certified_engine_pin("INV-033 engine-contract evidence");
}
