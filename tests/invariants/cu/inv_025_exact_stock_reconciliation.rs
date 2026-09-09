//! INV-025 - exact stock reconciliation.
//!
//! This file owns a compact public-route stock ledger for the wrapper boundary.
//! After each successful value-moving instruction it independently tracks the SPL
//! vault atoms and compares them with `MarketGroupV16::vault`, then checks senior
//! stock lower bounds and the backing bucket/source-credit mirror for the touched
//! domain. Rejected value-moving instructions must roll back both program state and
//! custody exactly.

use super::*;

#[path = "inv_025_active_reserve_swap.rs"]
mod active_reserve_swap;

#[test]
fn v16_program_fee_bearing_recovery_reconciles_raw_stocks_through_terminal_close() {
    use super::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const DEPOSITS: [u128; 2] = [1_000, 1_700];
    const INIT_FEE: u128 = 37;
    const INSURANCE: u128 = 71;
    const BACKING: [u128; 2] = [17, 29];
    const PRICE: u64 = 100;
    const LOTS: u128 = 3;
    const FEE_BPS: u64 = 1_000;
    const TRADE_FEE: u128 = LOTS * PRICE as u128 * FEE_BPS as u128 / 10_000;

    // Unlike the active-market census's derived nonnegative residual, this history
    // has a predetermined zero residual: no mark/funding change, lien or rounding.
    // It carries funded reserves and nonzero fees through Recovery and real exits.
    struct Stocks {
        capital: [Option<u128>; 2],
        insurance: [u128; 2],
        backing: [u128; 4],
        wallets: [u64; 3],
        supply: u64,
    }
    let mut expected = Stocks {
        capital: [None; 2],
        insurance: [0; 2],
        backing: [0; 4],
        wallets: [0; 3],
        supply: 0,
    };
    let mut env = inv018_public_spl_market(6);
    let owners = [Keypair::new(), Keypair::new()];
    let portfolio_keys = [Keypair::new(), Keypair::new()];
    let portfolios = portfolio_keys.each_ref().map(|key| key.pubkey());
    let admin = env.admin.insecure_clone();
    let wallets = [owners[0].pubkey(), owners[1].pubkey(), admin.pubkey()];
    let tokens =
        wallets.map(|wallet| create_ata_for_test(&mut env.svm, &env.payer, wallet, env.mint));

    let census = |env: &V16CuEnv, expected: &Stocks, label: &str| {
        let market = env.svm.get_account(&env.market).unwrap();
        let header = market_group_header_bytes(&market.data);
        let (_, decoded) = state::read_market(&market.data).unwrap();
        let mut capital = 0;
        let mut materialized = 0;
        for (index, amount) in expected.capital.iter().enumerate() {
            if let Some(amount) = amount {
                let account = env.portfolio_state(portfolios[index]);
                assert_eq!(account.capital.get(), *amount, "{label}: owner {index}");
                assert_eq!(account.pnl.get(), 0, "{label}: no mark or funding PnL");
                assert_eq!(account.cancel_deposit_escrow.get(), 0, "{label}: escrow");
                capital += account.capital.get();
                materialized += 1;
            }
        }
        let mut backing_num = 0;
        let mut earnings = 0;
        let mut insurance_budget = 0;
        for asset in 0..state::market_slot_capacity(&market.data).unwrap() {
            let slot = bytemuck::pod_read_unaligned::<percolator::EngineAssetSlotV16Account>(
                market_engine_slot_bytes(&market.data, asset),
            );
            let budget = slot.insurance_domain_budget_long.get()
                + slot.insurance_domain_budget_short.get()
                - slot.insurance_domain_spent_long.get()
                - slot.insurance_domain_spent_short.get();
            assert_eq!(
                budget, expected.insurance[asset],
                "{label}: asset {asset} insurance"
            );
            insurance_budget += budget;
            for (side, (bucket, source)) in [
                (slot.backing_long, slot.source_credit_long),
                (slot.backing_short, slot.source_credit_short),
            ]
            .into_iter()
            .enumerate()
            {
                let domain = 2 * asset + side;
                let fresh = bucket.fresh_unliened_backing_num.get();
                assert_eq!(
                    fresh,
                    expected.backing[domain] * BOUND_SCALE,
                    "{label}: domain {domain}"
                );
                assert_eq!(
                    source.fresh_reserved_backing_num.get(),
                    fresh,
                    "{label}: backing mirror"
                );
                for encumbrance in [
                    bucket.valid_liened_backing_num.get(),
                    bucket.impaired_liened_backing_num.get(),
                    bucket.consumed_liened_backing_num.get(),
                    source.positive_claim_bound_num.get(),
                    source.exact_positive_claim_num.get(),
                    source.spent_backing_num.get(),
                    source.provider_receivable_num.get(),
                    source.valid_liened_backing_num.get(),
                    source.impaired_liened_backing_num.get(),
                    source.insurance_credit_reserved_num.get(),
                ] {
                    assert_eq!(
                        encumbrance, 0,
                        "{label}: no hidden claim or lien in domain {domain}"
                    );
                }
                backing_num += fresh;
                earnings += bucket.utilization_fee_earnings.get();
            }
        }
        assert_eq!(
            earnings, 0,
            "{label}: no utilization earnings without liens"
        );
        assert_eq!(
            backing_num % BOUND_SCALE,
            0,
            "{label}: no fractional backing"
        );
        for (name, raw, decoded, scanned) in [
            ("capital", header.c_tot.get(), decoded.c_tot, capital),
            (
                "insurance",
                header.insurance.get(),
                decoded.insurance,
                insurance_budget,
            ),
            (
                "earnings",
                header.backing_provider_earnings_total.get(),
                decoded.backing_provider_earnings_total,
                earnings,
            ),
            (
                "budget",
                header.insurance_domain_budget_remaining_total.get(),
                decoded.insurance_domain_budget_remaining_total,
                insurance_budget,
            ),
            (
                "claims",
                header.source_claim_bound_total_num.get(),
                decoded.source_claim_bound_total_num,
                0,
            ),
            (
                "insurance reservations",
                header.source_insurance_credit_reserved_total_atoms.get(),
                decoded.source_insurance_credit_reserved_total_atoms,
                0,
            ),
            (
                "positive PnL",
                header.pnl_pos_tot.get(),
                decoded.pnl_pos_tot,
                0,
            ),
        ] {
            assert_eq!((raw, decoded), (scanned, scanned), "{label}: {name} census");
        }
        assert_eq!(
            header.materialized_portfolio_count.get(),
            materialized,
            "{label}: portfolios"
        );
        assert_eq!(
            header.source_fresh_backing_total_num.get(),
            backing_num,
            "{label}: fresh total"
        );
        let stock = capital + insurance_budget + earnings + backing_num / BOUND_SCALE;
        let vault = env.token_amount(env.vault);
        assert_eq!(
            (header.vault.get(), decoded.vault, u128::from(vault)),
            (stock, stock, stock),
            "{label}: exact partition, zero rounding residue and zero protocol surplus"
        );
        for (index, token) in tokens.iter().enumerate() {
            assert_eq!(
                env.token_amount(*token),
                expected.wallets[index],
                "{label}: wallet {index}"
            );
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, expected.supply, "{label}: issued atoms");
        assert_eq!(
            vault + tokens.iter().map(|key| env.token_amount(*key)).sum::<u64>(),
            mint.supply,
            "{label}: every issued atom remains in the vault or its owner's wallet"
        );
    };
    let mut steps = 0;
    let mut peak_cu = 0;
    macro_rules! step {
        ($label:literal, $action:expr) => {{
            let cu = $action;
            assert_cu_within($label, cu, TRADE_CU_LIMIT);
            peak_cu = peak_cu.max(cu);
            steps += 1;
            census(&env, &expected, $label);
        }};
    }
    step!("public genesis", env.init_market_cu);
    for (index, amount) in [
        DEPOSITS[0] + INIT_FEE,
        DEPOSITS[1],
        INSURANCE + BACKING.iter().sum::<u128>(),
    ]
    .into_iter()
    .enumerate()
    {
        expected.wallets[index] = amount as u64;
        expected.supply += amount as u64;
        step!(
            "public mint funding",
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[index],
                    &admin.pubkey(),
                    &[],
                    amount as u64
                )
                .unwrap()],
                &[&admin]
            )
            .unwrap()
        );
    }
    for index in 0..2 {
        step!(
            "System portfolio allocation",
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_keys[index],
                env.portfolio_account_len,
                env.program_id
            )
        );
        expected.capital[index] = Some(0);
        step!(
            "InitPortfolio",
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[index].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[index], false),
                ],
                &[&owners[index]]
            )
            .unwrap()
        );
        env.portfolios.push(portfolios[index]);
        expected.capital[index] = Some(DEPOSITS[index]);
        expected.wallets[index] -= DEPOSITS[index] as u64;
        step!(
            "Deposit",
            env.send(
                env.deposit_ix(portfolios[index], DEPOSITS[index]),
                vec![
                    AccountMeta::new(owners[index].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[index], false),
                    AccountMeta::new(tokens[index], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[index]]
            )
            .unwrap()
        );
    }
    expected.insurance[0] = INSURANCE;
    expected.wallets[2] -= INSURANCE as u64;
    step!(
        "TopUpInsurance",
        env.top_up_insurance_from_admin_token_with_cu(tokens[2], INSURANCE)
    );
    expected.backing[0] = BACKING[0];
    expected.wallets[2] -= BACKING[0] as u64;
    step!(
        "asset-0 backing",
        env.top_up_backing_bucket_from_admin_token_with_cu(tokens[2], 0, BACKING[0], 100)
    );
    step!(
        "activation fee policy",
        env.update_market_init_fee_policy_with_cu(INIT_FEE)
    );
    step!(
        "Recovery timeout policy",
        env.configure_permissionless_resolve_with_cu(100, 1)
    );

    env.svm.warp_to_slot(1);
    expected.insurance[0] += INIT_FEE;
    expected.wallets[0] -= INIT_FEE as u64;
    step!(
        "paid dynamic activation",
        env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                market_id: env.market_state().1.next_market_id,
                authority_epoch: 0,
                now_slot: 1,
                initial_price: PRICE,
                max_init_fee: INIT_FEE,
                insurance_authority: admin.pubkey().to_bytes(),
                insurance_operator: admin.pubkey().to_bytes(),
                backing_bucket_authority: admin.pubkey().to_bytes(),
                oracle_authority: admin.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[0]]
        )
        .unwrap()
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active
    );
    expected.backing[3] = BACKING[1];
    expected.wallets[2] -= BACKING[1] as u64;
    step!(
        "asset-1 backing",
        env.top_up_backing_bucket_from_admin_token_with_cu(tokens[2], 3, BACKING[1], 100)
    );
    expected.capital = DEPOSITS.map(|amount| Some(amount - TRADE_FEE));
    expected.insurance[1] = 2 * TRADE_FEE;
    step!(
        "fee-bearing TradeNoCpi",
        env.trade_asset_with_cu(
            1,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            (LOTS * POS_SCALE) as i128,
            PRICE,
            FEE_BPS
        )
    );
    assert_eq!(
        env.market_state().1.assets[1].oi_eff_long_q,
        LOTS * POS_SCALE
    );
    step!(
        "enter Recovery",
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 1, 1, 0)
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );
    env.svm.warp_to_slot(2);
    step!(
        "permissionless Recovery force-close",
        env.force_close_abandoned_asset_with_cu(
            &owners[1],
            portfolios[0],
            portfolios[1],
            1,
            2,
            LOTS * POS_SCALE
        )
    );
    for portfolio in portfolios {
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(portfolio),
            1
        ));
    }

    // Insurance and both providers still own real vault atoms. A recovered owner
    // cannot withdraw one of those atoms, and rejection must not poison its exit.
    let withdrawal_accounts = vec![
        AccountMeta::new(owners[0].pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolios[0], false),
        AccountMeta::new(tokens[0], false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    let capital = DEPOSITS[0] - TRADE_FEE;
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: withdrawal_accounts.clone(),
                data: env.withdraw_ix(portfolios[0], capital + 1).encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &owners[0]],
        env.svm.latest_blockhash(),
    );
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(portfolios);
    keys.extend(tokens);
    keys.push(env.mint);
    keys.sort_unstable();
    keys.dedup();
    let frame: Vec<_> = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect();
    let failure = env
        .svm
        .send_transaction(tx)
        .expect_err("one atom beyond owner capital must reject");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
        )
    );
    assert_cu_within(
        "recovered over-withdraw",
        failure.meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    for (key, mut before) in frame {
        if key == env.payer.pubkey() {
            before.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            env.svm.get_account(&key),
            before,
            "exact rejected account frame: {key}"
        );
    }
    census(&env, &expected, "rejected recovered over-withdraw");
    env.svm.expire_blockhash();
    expected.capital[0] = Some(0);
    expected.wallets[0] += capital as u64;
    step!(
        "fresh exact-capital Withdraw",
        env.send(
            env.withdraw_ix(portfolios[0], capital),
            withdrawal_accounts,
            &[&owners[0]]
        )
        .unwrap()
    );
    step!("ResolveMarket", env.resolve());
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    expected.capital[1] = Some(0);
    expected.wallets[1] += (DEPOSITS[1] - TRADE_FEE) as u64;
    step!(
        "signed CloseResolved",
        env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0
            },
            vec![
                AccountMeta::new(owners[1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[1], false),
                AccountMeta::new(tokens[1], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[1]]
        )
        .unwrap()
    );
    for index in 0..2 {
        expected.capital[index] = None;
        step!(
            "ClosePortfolio",
            env.close_portfolio_with_cu(&owners[index], portfolios[index])
        );
        if let Some(account) = env.svm.get_account(&portfolios[index]) {
            assert_eq!(account.lamports, 0);
            assert!(account.data.is_empty());
        }
    }
    for (domain, amount) in [(3, BACKING[1]), (0, BACKING[0])] {
        expected.backing[domain] = 0;
        expected.wallets[2] += amount as u64;
        step!(
            "terminal backing withdrawal",
            env.withdraw_backing_bucket_to_admin_token_with_cu(tokens[2], domain as u16, amount)
        );
    }
    for (asset, amount) in [(1, 2 * TRADE_FEE), (0, INSURANCE + INIT_FEE)] {
        expected.insurance[asset] = 0;
        expected.wallets[2] += amount as u64;
        step!(
            "terminal insurance withdrawal",
            env.withdraw_insurance_domain_to_admin_token_with_cu(
                tokens[2],
                2 * asset as u16,
                amount
            )
        );
    }
    let close_cu = env
        .send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("fully reconciled Recovery history must close");
    assert_cu_within("CloseSlab", close_cu, CUSTODY_CU_LIMIT);
    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
    if let Some(vault) = env.svm.get_account(&env.vault) {
        assert_eq!(vault.lamports, 0);
        assert!(vault.data.iter().all(|byte| *byte == 0));
    }
    for (index, token) in tokens.iter().enumerate() {
        assert_eq!(env.token_amount(*token), expected.wallets[index]);
    }
    let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
        .unwrap()
        .supply;
    assert_eq!(supply, expected.supply);
    assert_eq!(
        tokens.iter().map(|key| env.token_amount(*key)).sum::<u64>(),
        supply
    );
    println!("INV-025: {steps} stock checkpoints, peak {peak_cu} CU; rollback {} CU; CloseSlab {close_cu} CU", failure.meta.compute_units_consumed);
}

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

    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 0, 11_000, 100);
    assert_domain_backing_mirror(&env, 0, 11_000, "after backing top-up");
    assert_exact_stock(&env, 76_000, "after backing top-up");

    let backing_dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    env.withdraw_backing_bucket_with_ledger_to_admin_token_with_cu(ledger, backing_dest, 0, 4_000);
    assert_eq!(env.token_amount(backing_dest), 4_000);
    assert_domain_backing_mirror(&env, 0, 7_000, "after backing withdraw");
    assert_exact_stock(&env, 72_000, "after backing withdraw");
    let ledger_state =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 7_000);
    assert_eq!(ledger_state.total_deposited_atoms, 11_000);
    assert_eq!(ledger_state.total_principal_withdrawn_atoms, 4_000);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&backing_dest).unwrap();
    let admin = env.admin.insecure_clone();
    let market_id = env.asset_market_id(0);
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id,
            authority_epoch: 0,
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

#[test]
fn v16_program_insurance_ledger_profit_and_loss_follow_public_routes() {
    const INITIAL_INSURANCE: u128 = 1_000;
    const MARKET_INIT_FEE: u128 = 37;
    const INITIAL_PRICE: u64 = 100;

    let mut env = V16CuEnv::new();
    let ledger = env.insurance_ledger_account();
    env.top_up_insurance_with_ledger_with_cu(ledger, INITIAL_INSURANCE);
    let read_ledger = |env: &V16CuEnv| {
        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap()
    };
    assert_eq!(
        read_ledger(&env).last_observed_insurance_atoms,
        INITIAL_INSURANCE
    );

    // A permissionless asset-init fee is protocol earnings, not provider principal.
    env.update_market_init_fee_policy_with_cu(MARKET_INIT_FEE);
    env.svm.warp_to_slot(1);
    let creator = Keypair::new();
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        INITIAL_PRICE,
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        MARKET_INIT_FEE,
    );
    env.sync_insurance_ledger_with_cu(ledger);
    let after_profit = read_ledger(&env);
    assert_eq!(after_profit.cumulative_profit_atoms, MARKET_INIT_FEE);
    assert_eq!(after_profit.cumulative_loss_atoms, 0);
    assert_eq!(
        after_profit.last_observed_insurance_atoms,
        INITIAL_INSURANCE + MARKET_INIT_FEE,
    );

    // A publicly liquidated insolvent counterparty consumes real asset-0 insurance.
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 200);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        (2 * POS_SCALE) as i128,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 1_000);
    env.svm.warp_to_slot(4);
    env.crank_steps(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
        4,
    );
    let insurance_after_loss = env.market_state().1.insurance;
    assert!(
        insurance_after_loss < after_profit.last_observed_insurance_atoms,
        "public insolvency must spend nonzero insurance"
    );
    env.sync_insurance_ledger_with_cu(ledger);
    let after_loss = read_ledger(&env);
    assert_eq!(after_loss.cumulative_profit_atoms, MARKET_INIT_FEE);
    assert_eq!(
        after_loss.cumulative_loss_atoms,
        after_profit.last_observed_insurance_atoms - insurance_after_loss,
        "ledger loss equals the exact publicly consumed insurance stock"
    );
    assert_eq!(
        after_loss.last_observed_insurance_atoms,
        insurance_after_loss,
    );
    assert_eq!(
        env.token_amount(env.vault) as u128,
        env.market_state().1.vault
    );
}
