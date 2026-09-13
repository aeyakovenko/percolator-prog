//! INV-010/024/027/044/053/060/062/081: preexisting target lag blocks first
//! risk after elapsed maintenance, even when both portfolios share an owner.
//! Fee/refresh order and retained instruction-content retries preserve the exact
//! margin boundary and each portfolio's owner entitlement. Fees are synchronized
//! explicitly; standalone admission with uncollected flat fees is outside scope.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_snapshot_full_refresh, assert_market_stock_census,
    assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_first_risk_after_elapsed_fees_and_preexisting_lag() {
    const BIRTH: [u64; 2] = [1, 2];
    const OPEN: u64 = 6;
    const LAGGED_SIZE: i128 = (POS_SCALE + POS_SCALE / 10) as i128;
    const SIZE: i128 = (112 * POS_SCALE / 100) as i128;
    let fees = BIRTH.map(|slot| FEE_RATE * u128::from(OPEN - slot));
    let notional = |size: i128| (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    let lag = |size: i128| size.unsigned_abs().div_ceil(POS_SCALE);
    let requirement = |size| notional(size) + lag(size);
    let insurance = fees.iter().sum::<u128>();
    assert_eq!(
        (fees, notional(LAGGED_SIZE), lag(LAGGED_SIZE)),
        ([35, 28], 110, 2)
    );
    assert_eq!(
        (requirement(LAGGED_SIZE), requirement(LAGGED_SIZE + 1)),
        (112, 113)
    );
    assert_eq!((notional(SIZE), notional(SIZE + 1)), (112, 113));

    let mut worlds = 0;
    let mut peak_cu = 0;
    for thin in 0..2 {
        let equity = std::array::from_fn::<_, 2, _>(|i| if i == thin { 112 } else { 160 });
        let deposits = std::array::from_fn::<_, 2, _>(|i| equity[i] + fees[i]);
        assert!(deposits[thin] >= notional(SIZE + 1));
        assert_eq!(deposits[thin] - fees[thin], notional(SIZE));
        let supply = deposits.iter().sum::<u128>();
        let target = if thin == 0 { PRICE - 1 } else { PRICE + 1 };
        let mut reference_certificates = None;
        for common_owner in [false, true] {
            for batch in [false, true] {
                for reverse in [false, true] {
                    let label = format!(
                        "first risk/preexisting lag/thin={thin}/common={common_owner}/batch={batch}/reverse={reverse}"
                    );
                    let mut env = inv018_public_spl_market_with_params(
                        0,
                        V16CuMarketParams {
                            maintenance_fee_per_slot: FEE_RATE,
                            maintenance_margin_bps: 5_000,
                            initial_margin_bps: 10_000,
                            max_price_move_bps_per_slot: 500,
                            max_abs_funding_e9_per_slot: 0,
                            ..V16CuMarketParams::default()
                        },
                    );
                    env.svm.warp_to_slot(BIRTH[0]);
                    env.configure_auth_mark_for_asset_as_admin(0, BIRTH[0], PRICE);
                    let keys = [Keypair::new(), Keypair::new()];
                    let owners = [&keys[0], &keys[usize::from(!common_owner)]];
                    let keeper_owner = Keypair::new();
                    let keeper = public_portfolio(&mut env, &keeper_owner);
                    let mut portfolios = [Pubkey::default(); 2];
                    let mut tokens = [Pubkey::default(); 2];
                    for i in 0..2 {
                        env.svm.warp_to_slot(BIRTH[i]);
                        portfolios[i] = if common_owner && i == 1 {
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
                                    AccountMeta::new(owners[i].pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(key.pubkey(), false),
                                ],
                                &[owners[i]],
                            )
                            .unwrap();
                            env.portfolios.push(key.pubkey());
                            key.pubkey()
                        } else {
                            public_portfolio(&mut env, owners[i])
                        };
                        tokens[i] = if common_owner && i == 1 {
                            tokens[0]
                        } else {
                            create_ata_for_test(
                                &mut env.svm,
                                &env.payer,
                                owners[i].pubkey(),
                                env.mint,
                            )
                        };
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                &tokens[i],
                                &env.admin.pubkey(),
                                &[],
                                deposits[i].try_into().unwrap(),
                            )
                            .unwrap(),
                            &[&env.admin],
                        )
                        .unwrap();
                        env.send(
                            env.deposit_ix(portfolios[i], deposits[i]),
                            vec![
                                AccountMeta::new(owners[i].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                                AccountMeta::new(tokens[i], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[owners[i]],
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
                            &env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();

                    let trade = |env: &V16CuEnv, size_q| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[0].pubkey(), true),
                            AccountMeta::new(owners[1].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                            AccountMeta::new(portfolios[1], false),
                        ],
                        data: if batch {
                            env.batch_trade_no_cpi_ix(
                                portfolios[0],
                                portfolios[1],
                                vec![BatchTradeLeg {
                                    asset_index: 0,
                                    market_id: env.asset_market_id(0),
                                    size_q,
                                    exec_price: PRICE,
                                    fee_bps: 0,
                                }],
                            )
                        } else {
                            env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, size_q, PRICE, 0)
                        }
                        .encode(),
                    };
                    // Keep the request contents made before either elapsed fees or raw lag.
                    let exact = trade(&env, SIZE);
                    let excess = trade(&env, SIZE + 1);
                    let lagged = [trade(&env, LAGGED_SIZE), trade(&env, LAGGED_SIZE + 1)];
                    let funded = portfolios.map(|key| env.svm.get_account(&key));
                    for slot in BIRTH[1]..=OPEN {
                        env.svm.warp_to_slot(slot);
                        env.crank(
                            keeper,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations(0),
                            },
                        );
                    }
                    env.push_auth_mark_for_asset_as_admin(0, OPEN, target);
                    env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: OPEN,
                            observations: crank_observations(0),
                        },
                    );
                    assert_eq!(
                        portfolios.map(|key| env.svm.get_account(&key)),
                        funded,
                        "{label}"
                    );
                    let staged = env.market_state().1;
                    assert_eq!(
                        (staged.current_slot, staged.assets[0].slot_last),
                        (OPEN, OPEN)
                    );
                    assert_eq!(
                        (
                            staged.assets[0].effective_price,
                            staged.assets[0].raw_oracle_target_price
                        ),
                        (PRICE, target)
                    );
                    assert_eq!(
                        (
                            staged.assets[0].oi_eff_long_q,
                            staged.assets[0].oi_eff_short_q
                        ),
                        (0, 0)
                    );

                    let stable = [
                        env.mint,
                        owners[0].pubkey(),
                        owners[1].pubkey(),
                        keeper_owner.pubkey(),
                        env.admin.pubkey(),
                        env.vault_authority,
                    ];
                    let stable_before = stable.map(|key| env.svm.get_account(&key));
                    let mut tracked = stable.to_vec();
                    tracked.extend([
                        keeper,
                        env.market,
                        env.vault,
                        portfolios[0],
                        portfolios[1],
                        tokens[0],
                        tokens[1],
                    ]);
                    let check = |env: &V16CuEnv,
                                 collected: bool,
                                 open: bool,
                                 paid: [u128; 2],
                                 expected_target: u64| {
                        let market = env.svm.get_account(&env.market).unwrap();
                        let group = env.market_state().1;
                        let accounts = [portfolios[0], portfolios[1], keeper]
                            .map(|key| env.portfolio_state(key));
                        let charged = if collected { fees } else { [0; 2] };
                        assert_eq!(group.insurance, charged.iter().sum::<u128>(), "{label}");
                        assert_eq!(
                            group.c_tot,
                            supply - group.insurance - paid.iter().sum::<u128>()
                        );
                        assert_eq!(group.vault, supply - paid.iter().sum::<u128>());
                        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                        assert_eq!(group.vault, group.c_tot + group.insurance);
                        assert_eq!(
                            (group.pnl_pos_tot, group.source_claim_bound_total_num),
                            (0, 0)
                        );
                        assert_eq!(
                            group.insurance_domain_budget[0],
                            charged.iter().map(|fee| fee / 2).sum::<u128>()
                        );
                        assert_eq!(
                            group.insurance_domain_budget[1],
                            charged.iter().map(|fee| fee - fee / 2).sum::<u128>()
                        );
                        assert!(group.insurance_domain_budget[2..]
                            .iter()
                            .all(|&amount| amount == 0));
                        assert_eq!(
                            (
                                group.assets[0].effective_price,
                                group.assets[0].raw_oracle_target_price
                            ),
                            (PRICE, expected_target)
                        );
                        let oi = if open { SIZE as u128 } else { 0 };
                        assert_eq!(
                            (
                                group.assets[0].oi_eff_long_q,
                                group.assets[0].oi_eff_short_q
                            ),
                            (oi, oi)
                        );
                        assert_eq!(
                            (
                                group.assets[0].k_long,
                                group.assets[0].k_short,
                                group.assets[0].f_long_num,
                                group.assets[0].f_short_num
                            ),
                            (
                                staged.assets[0].k_long,
                                staged.assets[0].k_short,
                                staged.assets[0].f_long_num,
                                staged.assets[0].f_short_num
                            )
                        );
                        for i in 0..2 {
                            let account = &accounts[i];
                            assert_eq!(
                                account.capital.get(),
                                deposits[i] - charged[i] - paid[i],
                                "{label}: portfolio {i}"
                            );
                            assert_eq!(
                                account.last_fee_slot.get(),
                                if collected { OPEN } else { BIRTH[i] }
                            );
                            assert_eq!((account.fee_credits.get(), account.pnl.get()), (0, 0));
                            assert_eq!(
                                percolator::active_bitmap_count_ones(active_bitmap(account)),
                                u32::from(open)
                            );
                            assert_eq!(
                                u128::from(env.token_amount(tokens[i])),
                                if common_owner {
                                    paid.iter().sum()
                                } else {
                                    paid[i]
                                }
                            );
                            let current = assert_current_certificate_matches_independent(
                                &label, &group, account,
                            )
                            .unwrap();
                            if open {
                                assert!(current, "{label}: both first opens must certify");
                                let cert = health_cert(account);
                                assert_eq!(cert.certified_equity, equity[i] as i128);
                                assert_eq!(cert.certified_initial_req, notional(SIZE));
                                assert_eq!(
                                    cert.certified_maintenance_req,
                                    notional(SIZE).div_ceil(2)
                                );
                                assert_eq!(cert.certified_worst_case_loss, notional(SIZE));
                                assert_eq!(cert.certified_liq_deficit, 0);
                                let leg = active_leg_for_asset(account, 0);
                                assert_eq!(leg.basis_pos_q.unsigned_abs(), SIZE as u128);
                                assert_eq!(
                                    leg.side,
                                    if i == 0 {
                                        SideV16::Long
                                    } else {
                                        SideV16::Short
                                    }
                                );
                                assert!(assert_current_certificate_matches_snapshot_full_refresh(
                                    &label,
                                    &market.data,
                                    &env.svm.get_account(&portfolios[i]).unwrap().data,
                                )
                                .unwrap());
                            }
                        }
                        assert_eq!((accounts[2].capital.get(), accounts[2].pnl.get()), (0, 0));
                        assert_eq!(stable.map(|key| env.svm.get_account(&key)), stable_before);
                        assert_market_stock_census(
                            &label,
                            &group,
                            &market.data,
                            &accounts,
                            group.vault,
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                        assert_source_credit_rates(&label, &group).unwrap();
                    };
                    check(&env, false, false, [0; 2], target);

                    let mut submit = |env: &mut V16CuEnv,
                                      instructions: Vec<Instruction>,
                                      rejection: Option<(u8, PercolatorError)>,
                                      limit| {
                        env.svm.expire_blockhash();
                        let mut all = vec![heap_ix(), cu_ix()];
                        all.extend(instructions);
                        let mut signers = vec![&env.payer];
                        for signer in [owners[0], owners[1], &keeper_owner] {
                            if !signers.iter().any(|old| old.pubkey() == signer.pubkey())
                                && all.iter().any(|ix| {
                                    ix.accounts.iter().any(|meta| {
                                        meta.is_signer && meta.pubkey == signer.pubkey()
                                    })
                                })
                            {
                                signers.push(signer);
                            }
                        }
                        let tx = Transaction::new_signed_with_payer(
                            &all,
                            Some(&env.payer.pubkey()),
                            &signers,
                            env.svm.latest_blockhash(),
                        );
                        tx.verify().unwrap();
                        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                        let mut frame = tracked.clone();
                        frame.extend(tx.message.account_keys.iter().copied());
                        frame.sort_unstable();
                        frame.dedup();
                        let mut before: Vec<_> =
                            frame.iter().map(|key| env.svm.get_account(key)).collect();
                        let network_fee = u64::from(tx.message.header.num_required_signatures)
                            * FeeStructure::default().lamports_per_signature;
                        let cu = if let Some((index, error)) = rejection {
                            let failed = env
                                .svm
                                .send_transaction(tx)
                                .expect_err("public conformance rejection");
                            assert_eq!(
                                failed.err,
                                TransactionError::InstructionError(
                                    index,
                                    InstructionError::Custom(error as u32)
                                ),
                                "{label}"
                            );
                            let payer = frame
                                .iter()
                                .position(|key| *key == env.payer.pubkey())
                                .unwrap();
                            before[payer].as_mut().unwrap().lamports -= network_fee;
                            assert_eq!(
                                frame
                                    .iter()
                                    .map(|key| env.svm.get_account(key))
                                    .collect::<Vec<_>>(),
                                before,
                                "{label}: exact transaction rollback"
                            );
                            failed.meta.compute_units_consumed
                        } else {
                            env.svm
                                .send_transaction(tx)
                                .expect("public conformance success")
                                .compute_units_consumed
                        };
                        assert_cu_within(&label, cu, limit);
                        peak_cu = peak_cu.max(cu);
                    };
                    let order = if reverse { [1, 0] } else { [0, 1] };
                    let mut prefix = Vec::new();
                    for i in order {
                        prefix.push(Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            data: ProgInstruction::SyncMaintenanceFee { now_slot: OPEN }.encode(),
                        });
                        prefix.push(Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(keeper_owner.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            data: ProgInstruction::PermissionlessCrank {
                                now_slot: OPEN,
                                observations: crank_observations(0),
                            }
                            .encode(),
                        });
                    }
                    let withdrawal = |env: &V16CuEnv, i: usize, amount| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.withdraw_ix(portfolios[i], amount).encode(),
                    };
                    for request in lagged {
                        let mut blocked = prefix.clone();
                        blocked.push(request);
                        submit(
                            &mut env,
                            blocked,
                            Some((6, PercolatorError::EngineLockActive)),
                            750_000,
                        );
                        check(&env, false, false, [0; 2], target);
                    }
                    // The traded-asset gate precedes numerical margin. Clear it publicly,
                    // without exposing either owner or changing its retained request contents.
                    env.push_auth_mark_for_asset_as_admin(0, OPEN, PRICE);
                    env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: OPEN,
                            observations: crank_observations(0),
                        },
                    );
                    check(&env, false, false, [0; 2], PRICE);
                    let mut above = prefix.clone();
                    above.push(excess);
                    submit(
                        &mut env,
                        above,
                        Some((6, PercolatorError::EngineInvalidConfig)),
                        750_000,
                    );
                    check(&env, false, false, [0; 2], PRICE);
                    let mut admission = prefix.clone();
                    admission.push(exact);
                    submit(&mut env, admission, None, 750_000);
                    check(&env, true, true, [0; 2], PRICE);
                    let certificates = portfolios.map(|key| health_cert(&env.portfolio_state(key)));
                    assert_eq!(
                        certificates[thin].certified_equity as u128,
                        certificates[thin].certified_initial_req
                    );
                    if let Some(expected) = &reference_certificates {
                        assert_eq!(
                            &certificates, expected,
                            "{label}: owner identity, route and fee order cannot change health"
                        );
                    } else {
                        reference_certificates = Some(certificates);
                    }

                    // Same-slot synchronization cannot charge either portfolio a second time.
                    let settled = tracked
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>();
                    submit(
                        &mut env,
                        vec![prefix[0].clone(), prefix[2].clone()],
                        None,
                        CUSTODY_CU_LIMIT,
                    );
                    assert_eq!(
                        tracked
                            .iter()
                            .map(|key| env.svm.get_account(key))
                            .collect::<Vec<_>>(),
                        settled
                    );
                    check(&env, true, true, [0; 2], PRICE);
                    let close = trade(&env, -SIZE);
                    let overdraw = withdrawal(&env, thin, equity[thin] + 1);
                    submit(
                        &mut env,
                        vec![close.clone(), overdraw],
                        Some((3, PercolatorError::EngineLockActive)),
                        TRADE_CU_LIMIT + CUSTODY_CU_LIMIT,
                    );
                    check(&env, true, true, [0; 2], PRICE);
                    submit(&mut env, vec![close], None, TRADE_CU_LIMIT);
                    check(&env, true, false, [0; 2], PRICE);
                    let mut paid = [0; 2];
                    for i in order {
                        let payout = withdrawal(&env, i, equity[i]);
                        submit(&mut env, vec![payout], None, CUSTODY_CU_LIMIT);
                        paid[i] = equity[i];
                        check(&env, true, false, paid, PRICE);
                    }
                    assert_eq!(env.market_state().1.c_tot, 0);
                    assert_eq!(u128::from(env.token_amount(env.vault)), insurance);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("first risk/preexisting lag: worlds={worlds}, exact_rollbacks={}, boundary_admissions={worlds}, owner_payouts={}, insurance_per_world={insurance}, peak_cu={peak_cu}", 4 * worlds, 2 * worlds);
}
