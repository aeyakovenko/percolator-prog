//! INV-027 - Protected principal seniority.
//!
//! Normative obligation: Junior value and fees cannot outrank protected principal or pending senior obligations.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions):
//! `v16_bpf_public_crystallized_loss_budget_credits_only_fresh_lp_principal`, the issue-408
//! standing-matcher/liquidation matrices, and
//! `v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit`. The latter
//! generates real provider earnings and live insurance, makes their asset locally loss-stale by
//! advancing another asset, and proves backing principal, provider earnings, and insurance all
//! reject with exact rollback while an unrelated flat user retains a complete public exit. The
//! source-complete census composes this matrix with the trade, conversion, reduction, crank, and
//! terminal witnesses and fails when the pinned wrapper adds an unclassified ingress.
//! The stale-positive-PnL withdrawal test composes implicit maintenance collection with a real
//! released claim: rejected partial withdrawals roll back, and principal exits before conversion.
//! The recovery-forfeit sibling separates an already booked junior claim from a later unrefreshed
//! gain: owner forfeiture may discard only the latter, while every senior principal atom exits
//! independently of whether an unrelated depositor withdraws before or after recovery cleanup.
//! The maintenance-terminal sibling orders a live senior withdrawal against another owner's fee
//! accrual and resolution. Live withdrawal and terminal payout return the same independently
//! computed principal, and exact-rollback insurance probes enforce user disposition before fees
//! leave custody, including the zero-capital, still-materialized-portfolio boundary.
//! The joint-admission sibling composes uncollected maintenance with rounded nontraded target
//! lag at an exact risk boundary, comparing direct admission to public crank settlement on
//! all four transports and either constrained party. It does not cover flat first-risk fees.
//! The flat first-admission transaction below covers explicit public fee synchronization and
//! refresh, including rollback of that prefix and exact owner payouts after admission. It does
//! not certify a standalone first open with uncollected fees (reopening 413 remains open).
//! The withdrawal-prefix first-admission test instead realizes flat fees implicitly while SPL
//! pays both owners: a later margin rejection must undo those transfers and fee debits together.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[path = "inv_027_recovery_forfeit_seniority.rs"]
mod recovery_forfeit_seniority;

#[path = "inv_027_maintenance_terminal_seniority.rs"]
mod maintenance_terminal_seniority;

#[path = "inv_027_joint_admission_liabilities.rs"]
mod joint_admission_liabilities;

#[path = "inv_027_flat_admission_liabilities.rs"]
mod flat_admission_liabilities;

const ISSUE408_FEE_PER_SLOT: u128 = 1_000;
const ISSUE408_AGED_SLOT: u64 = 500;
const ISSUE408_MOVE_SLOT: u64 = 542;
const ISSUE408_LOTS: i128 = 10;

#[test]
fn v16_program_flat_first_admission_fee_prefix_is_atomic_and_entitled() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::support::fuzz_model::{
        assert_current_certificate_matches_independent, assert_market_stock_census,
        assert_reservation_encumbrance_census,
    };
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const START: u64 = 1;
    const ADMISSION: u64 = 4;
    const PRICE: u64 = 100;
    const RATE: u128 = 7;
    const FEE: u128 = (ADMISSION - START) as u128 * RATE;
    const DEPOSITS: [u128; 2] = [100 + FEE, 211 + FEE];
    const SIZE: i128 = POS_SCALE as i128;
    const EXCESS: i128 = SIZE + (POS_SCALE / PRICE as u128) as i128;

    let requirement = |size: i128| (size.unsigned_abs() * PRICE as u128).div_ceil(POS_SCALE);
    assert_eq!(
        (FEE, requirement(SIZE), requirement(EXCESS)),
        (21, 100, 101)
    );
    assert!(DEPOSITS[0] > requirement(EXCESS));
    assert_eq!(DEPOSITS[0] - FEE, requirement(SIZE));

    let mut certificates = None;
    let mut peak_cu = 0;
    for atomic in [true, false] {
        let label = format!("flat first admission/atomic={atomic}");
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                maintenance_fee_per_slot: RATE,
                maintenance_margin_bps: 5_000,
                initial_margin_bps: 10_000,
                max_price_move_bps_per_slot: 500,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(START);
        env.configure_auth_mark_for_asset_as_admin(0, START, PRICE);
        let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap();
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let tokens = std::array::from_fn::<_, 2, _>(|party| {
            let token =
                create_ata_for_test(&mut env.svm, &env.payer, owners[party].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    DEPOSITS[party].try_into().unwrap(),
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[party], DEPOSITS[party]),
                vec![
                    AccountMeta::new(owners[party].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[party], false),
                    AccountMeta::new(token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[party]],
            )
            .unwrap();
            token
        });
        let untouched = portfolios[..2]
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        for slot in START + 1..=ADMISSION {
            env.svm.warp_to_slot(slot);
            env.crank(
                portfolios[2],
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
        assert_eq!(
            portfolios[..2]
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            untouched,
            "{label}: neither funded owner was refreshed while flat"
        );
        let group = env.market_state().1;
        assert_eq!(group.current_slot, ADMISSION);
        assert_eq!(group.assets[0].slot_last, ADMISSION);
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].raw_oracle_target_price, PRICE);

        let tracked = [
            env.market,
            portfolios[0],
            portfolios[1],
            portfolios[2],
            env.vault,
            env.mint,
            tokens[0],
            tokens[1],
            owners[0].pubkey(),
            owners[1].pubkey(),
            owners[2].pubkey(),
            env.admin.pubkey(),
            env.vault_authority,
            env.program_id,
            spl_token::ID,
        ];
        let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
        let frame = snapshot(&env);
        let check = |env: &V16CuEnv, charged: [bool; 2], open: bool, paid: [u128; 2]| {
            let group = env.market_state().1;
            let accounts = portfolios.map(|key| env.portfolio_state(key));
            let total = DEPOSITS.iter().sum::<u128>();
            let fees = charged.iter().filter(|&&yes| yes).count() as u128 * FEE;
            assert_eq!(group.insurance, fees, "{label}");
            assert_eq!(
                group.c_tot,
                total - fees - paid.iter().sum::<u128>(),
                "{label}"
            );
            assert_eq!(group.vault, total - paid.iter().sum::<u128>(), "{label}");
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                group.vault,
                "{label}"
            );
            assert_eq!(group.vault, group.c_tot + group.insurance, "{label}");
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.source_claim_bound_total_num, 0);
            let payers = fees / FEE;
            assert_eq!(group.insurance_domain_budget[0], payers * (FEE / 2));
            assert_eq!(group.insurance_domain_budget[1], payers * (FEE - FEE / 2));
            for party in 0..2 {
                let account = &accounts[party];
                let fee = if charged[party] { FEE } else { 0 };
                assert_eq!(
                    account.capital.get(),
                    DEPOSITS[party] - fee - paid[party],
                    "{label}"
                );
                assert_eq!(
                    account.last_fee_slot.get(),
                    if charged[party] { ADMISSION } else { START }
                );
                assert_eq!(account.fee_credits.get(), 0);
                assert_eq!(account.pnl.get(), 0);
                assert_eq!(u128::from(env.token_amount(tokens[party])), paid[party]);
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(account)),
                    u32::from(open)
                );
                if open {
                    let leg = active_leg_for_asset(account, 0);
                    assert_eq!(leg.basis_pos_q.unsigned_abs(), SIZE as u128);
                    assert_eq!(
                        leg.side,
                        if party == 0 {
                            SideV16::Long
                        } else {
                            SideV16::Short
                        }
                    );
                }
            }
            assert_eq!(
                group.assets[0].oi_eff_long_q,
                if open { SIZE as u128 } else { 0 }
            );
            assert_eq!(
                group.assets[0].oi_eff_short_q,
                if open { SIZE as u128 } else { 0 }
            );
            assert_market_stock_census(
                &label,
                &group,
                &env.svm.get_account(&env.market).unwrap().data,
                &accounts,
                env.token_amount(env.vault).into(),
            )
            .unwrap();
            assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
            for (i, key) in tracked.iter().enumerate() {
                if ![
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    env.vault,
                    tokens[0],
                    tokens[1],
                ]
                .contains(key)
                {
                    assert_eq!(
                        env.svm.get_account(key),
                        frame[i],
                        "{label}: untouched {key}"
                    );
                }
            }
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                    .unwrap()
                    .supply as u128,
                total
            );
        };
        check(&env, [false; 2], false, [0; 2]);

        let program_id = env.program_id;
        let ix = |instruction: ProgInstruction, accounts| Instruction {
            program_id,
            accounts,
            data: instruction.encode(),
        };
        let mut prefix = Vec::new();
        for portfolio in &portfolios[..2] {
            prefix.push(ix(
                ProgInstruction::SyncMaintenanceFee {
                    now_slot: ADMISSION,
                },
                vec![
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(*portfolio, false),
                ],
            ));
            prefix.push(ix(
                ProgInstruction::PermissionlessCrank {
                    now_slot: ADMISSION,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(owners[2].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(*portfolio, false),
                ],
            ));
        }
        let trade = |env: &V16CuEnv, size| {
            ix(
                env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, size, PRICE, 0),
                vec![
                    AccountMeta::new(owners[0].pubkey(), true),
                    AccountMeta::new(owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[1], false),
                ],
            )
        };
        let too_large = trade(&env, EXCESS);
        let exact = trade(&env, SIZE);
        let submit = |env: &mut V16CuEnv, instructions: &[Instruction]| {
            env.svm.expire_blockhash();
            let mut all = vec![heap_ix(), cu_ix()];
            all.extend_from_slice(instructions);
            let tx = Transaction::new_signed_with_payer(
                &all,
                Some(&env.payer.pubkey()),
                &[&env.payer, &owners[0], &owners[1], &owners[2]][..],
                env.svm.latest_blockhash(),
            );
            env.svm.send_transaction(tx)
        };
        let mut attempt = prefix.clone();
        attempt.push(too_large);
        let before = snapshot(&env);
        let failure = submit(&mut env, &attempt).expect_err("first risk must fit post-fee equity");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                6,
                InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)
            ),
            "{label}: {failure:?}"
        );
        assert_eq!(
            snapshot(&env),
            before,
            "{label}: successful fee/refresh prefix rolls back too"
        );
        check(&env, [false; 2], false, [0; 2]);
        peak_cu = peak_cu.max(failure.meta.compute_units_consumed);

        if atomic {
            attempt.pop();
            attempt.push(exact);
            let meta = submit(&mut env, &attempt).expect("complete first-admission transaction");
            peak_cu = peak_cu.max(meta.compute_units_consumed);
        } else {
            for (index, instruction) in prefix.iter().enumerate() {
                let signers: &[&Keypair] = if index % 2 == 0 { &[] } else { &[&owners[2]] };
                env.svm.expire_blockhash();
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![heap_ix(), cu_ix(), instruction.clone()],
                    signers,
                )
                .unwrap();
                check(&env, [true, index >= 2], false, [0; 2]);
                if index % 2 == 1 {
                    let account = env.portfolio_state(portfolios[index / 2]);
                    assert!(assert_current_certificate_matches_independent(
                        &label,
                        &env.market_state().1,
                        &account,
                    )
                    .unwrap());
                }
            }
            env.svm.expire_blockhash();
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![heap_ix(), cu_ix(), exact],
                &[&owners[0], &owners[1]],
            )
            .unwrap();
        }
        check(&env, [true; 2], true, [0; 2]);
        let certs = std::array::from_fn::<_, 2, _>(|party| {
            let account = env.portfolio_state(portfolios[party]);
            let cert = health_cert(&account);
            assert!(assert_current_certificate_matches_independent(
                &label,
                &env.market_state().1,
                &account
            )
            .unwrap());
            assert_eq!(cert.certified_equity, (DEPOSITS[party] - FEE) as i128);
            assert_eq!(cert.certified_initial_req, requirement(SIZE));
            assert_eq!(cert.certified_maintenance_req, requirement(SIZE) / 2);
            assert_eq!(cert.certified_liq_deficit, 0);
            cert
        });
        assert_eq!(
            certs[0].certified_equity,
            certs[0].certified_initial_req as i128
        );
        if let Some(expected) = certificates {
            assert_eq!(certs, expected, "atomic and committed public refresh agree");
        } else {
            certificates = Some(certs);
        }
        let close = trade(&env, -SIZE);
        env.svm.expire_blockhash();
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![heap_ix(), cu_ix(), close],
            &[&owners[0], &owners[1]],
        )
        .unwrap();
        check(&env, [true; 2], false, [0; 2]);
        let mut paid = [0; 2];
        for party in 0..2 {
            let amount = DEPOSITS[party] - FEE;
            env.send(
                env.withdraw_ix(portfolios[party], amount),
                vec![
                    AccountMeta::new(owners[party].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[party], false),
                    AccountMeta::new(tokens[party], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[party]],
            )
            .expect("owner receives only principal remaining after the pre-admission liability");
            paid[party] = amount;
            check(&env, [true; 2], false, paid);
        }
        assert_eq!(env.token_amount(env.vault) as u128, 2 * FEE);
    }
    assert_cu_within("flat first-admission transaction", peak_cu, 1_400_000);
    println!("INV-027 flat first admission: 2 prefix rollbacks, 2 admissions, 4 exact payouts; peak bundle CU={peak_cu}");
}

#[test]
fn v16_program_flat_withdrawal_fees_precede_first_admission_and_roll_back_custody() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::support::fuzz_model::{
        assert_current_certificate_matches_independent, assert_market_stock_census,
        assert_reservation_encumbrance_census,
    };
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const START: u64 = 1;
    const ADMISSION: u64 = 4;
    const RATE: u128 = 7;
    const FEE: u128 = (ADMISSION - START) as u128 * RATE;
    const PRICE: u64 = 100;
    const SIZE: i128 = POS_SCALE as i128;
    const EXCESS: i128 = SIZE + (POS_SCALE / PRICE as u128) as i128;
    let requirement = |size: i128| (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    assert_eq!(
        (FEE, requirement(SIZE), requirement(EXCESS)),
        (21, 100, 101)
    );

    let mut peak_cu = 0;
    for thin_party in 0..2 {
        let label = format!("flat withdrawal/first admission/thin={thin_party}");
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                maintenance_fee_per_slot: RATE,
                maintenance_margin_bps: 5_000,
                initial_margin_bps: 10_000,
                max_price_move_bps_per_slot: 500,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(START);
        env.configure_auth_mark_for_asset_as_admin(0, START, PRICE);
        let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap();
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let remaining = [0, 1].map(|party| if party == thin_party { 100u128 } else { 200 });
        let early_payout = [7u128, 11];
        let deposits = [0, 1].map(|party| remaining[party] + early_payout[party] + FEE);
        assert!(deposits[thin_party] - early_payout[thin_party] >= requirement(EXCESS));
        assert_eq!(
            deposits[thin_party] - early_payout[thin_party] - FEE,
            requirement(SIZE),
            "omitting elapsed fees would admit the extra atom of risk"
        );
        let tokens = [0, 1].map(|party| {
            let token =
                create_ata_for_test(&mut env.svm, &env.payer, owners[party].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    deposits[party].try_into().unwrap(),
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[party], deposits[party]),
                vec![
                    AccountMeta::new(owners[party].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[party], false),
                    AccountMeta::new(token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[party]],
            )
            .unwrap();
            token
        });
        let funded = portfolios[..2]
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>();
        for slot in START + 1..=ADMISSION {
            env.svm.warp_to_slot(slot);
            env.crank(
                portfolios[2],
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
        assert_eq!(
            portfolios[..2]
                .iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>(),
            funded,
            "{label}: public market progress left both never-traded fee cursors untouched"
        );

        let tracked = [
            env.market,
            portfolios[0],
            portfolios[1],
            portfolios[2],
            env.vault,
            env.mint,
            tokens[0],
            tokens[1],
            owners[0].pubkey(),
            owners[1].pubkey(),
            owners[2].pubkey(),
            env.admin.pubkey(),
            env.vault_authority,
            env.program_id,
            spl_token::ID,
        ];
        let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
        let frame = snapshot(&env);
        let check = |env: &V16CuEnv, charged: bool, open: bool, paid: [u128; 2]| {
            let group = env.market_state().1;
            let accounts = portfolios.map(|key| env.portfolio_state(key));
            let fee = if charged { FEE } else { 0 };
            let total = deposits.iter().sum::<u128>();
            let payout = paid.iter().sum::<u128>();
            assert_eq!(group.current_slot, ADMISSION);
            assert_eq!(group.assets[0].slot_last, ADMISSION);
            assert_eq!(group.assets[0].effective_price, PRICE);
            assert_eq!(group.assets[0].raw_oracle_target_price, PRICE);
            assert_eq!(group.insurance, 2 * fee, "{label}");
            assert_eq!(group.insurance_domain_budget[0], 2 * (fee / 2));
            assert_eq!(group.insurance_domain_budget[1], 2 * (fee - fee / 2));
            assert_eq!(group.c_tot, total - payout - 2 * fee, "{label}");
            assert_eq!(group.vault, total - payout);
            assert_eq!(group.vault, group.c_tot + group.insurance);
            assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.source_claim_bound_total_num, 0);
            for party in 0..2 {
                let account = &accounts[party];
                assert_eq!(
                    account.capital.get(),
                    deposits[party] - fee - paid[party],
                    "{label}"
                );
                assert_eq!(
                    account.last_fee_slot.get(),
                    if charged { ADMISSION } else { START }
                );
                assert_eq!(account.fee_credits.get(), 0);
                assert_eq!(account.pnl.get(), 0);
                assert_eq!(u128::from(env.token_amount(tokens[party])), paid[party]);
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(account)),
                    u32::from(open)
                );
                if open {
                    let leg = active_leg_for_asset(account, 0);
                    assert_eq!(leg.basis_pos_q.unsigned_abs(), SIZE as u128);
                    assert_eq!(
                        leg.side,
                        if party == 0 {
                            SideV16::Long
                        } else {
                            SideV16::Short
                        }
                    );
                }
            }
            assert_eq!(
                group.assets[0].oi_eff_long_q,
                if open { SIZE as u128 } else { 0 }
            );
            assert_eq!(
                group.assets[0].oi_eff_short_q,
                if open { SIZE as u128 } else { 0 }
            );
            assert_market_stock_census(
                &label,
                &group,
                &env.svm.get_account(&env.market).unwrap().data,
                &accounts,
                env.token_amount(env.vault).into(),
            )
            .unwrap();
            assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
            for (index, key) in tracked.iter().enumerate() {
                if ![
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    env.vault,
                    tokens[0],
                    tokens[1],
                ]
                .contains(key)
                {
                    assert_eq!(
                        env.svm.get_account(key),
                        frame[index],
                        "{label}: untouched {key}"
                    );
                }
            }
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                    .unwrap()
                    .supply as u128,
                total
            );
        };
        check(&env, false, false, [0; 2]);

        let withdraw = |env: &V16CuEnv, party: usize, amount| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(owners[party].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[party], false),
                AccountMeta::new(tokens[party], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env.withdraw_ix(portfolios[party], amount).encode(),
        };
        let trade = |env: &V16CuEnv, size| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(owners[1].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[0], false),
                AccountMeta::new(portfolios[1], false),
            ],
            data: env
                .trade_no_cpi_ix(portfolios[0], portfolios[1], 0, size, PRICE, 0)
                .encode(),
        };
        let submit = |env: &mut V16CuEnv, instructions: Vec<Instruction>| {
            env.svm.expire_blockhash();
            let mut all = vec![heap_ix(), cu_ix()];
            all.extend(instructions);
            let tx = Transaction::new_signed_with_payer(
                &all,
                Some(&env.payer.pubkey()),
                &[&env.payer, &owners[0], &owners[1]],
                env.svm.latest_blockhash(),
            );
            tx.verify().unwrap();
            assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
            assert_eq!(tx.message.header.num_required_signatures, 3);
            let mut expected_payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
            expected_payer.lamports -=
                3 * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
            let result = env.svm.send_transaction(tx);
            assert_eq!(
                env.svm.get_account(&env.payer.pubkey()),
                Some(expected_payer)
            );
            result
        };
        let prefix = [0, 1].map(|party| withdraw(&env, party, early_payout[party]));
        let mut attempt = prefix.to_vec();
        attempt.push(trade(&env, EXCESS));
        let failure = submit(&mut env, attempt)
            .expect_err("first risk must fit equity after withdrawals and elapsed fees");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(4, InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)),
            "{label}: both token-moving withdrawals must succeed before the admission rejects: {failure:?}"
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", spl_token::ID))
                .count(),
            2,
            "{label}: both SPL transfers executed before the failed first open"
        );
        peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
        assert_eq!(
            snapshot(&env),
            frame,
            "{label}: SPL payouts, fees, certificates and lamports all roll back"
        );
        check(&env, false, false, [0; 2]);

        let mut retry = prefix.to_vec();
        retry.push(trade(&env, SIZE));
        let success = submit(&mut env, retry)
            .expect("implicit withdrawal fee collection leaves exactly enough first-risk equity");
        peak_cu = peak_cu.max(success.compute_units_consumed);
        check(&env, true, true, early_payout);
        for party in 0..2 {
            let account = env.portfolio_state(portfolios[party]);
            assert!(assert_current_certificate_matches_independent(
                &label,
                &env.market_state().1,
                &account
            )
            .unwrap());
            let cert = health_cert(&account);
            assert_eq!(cert.certified_equity, remaining[party] as i128);
            assert_eq!(cert.certified_initial_req, requirement(SIZE));
            assert_eq!(cert.certified_maintenance_req, requirement(SIZE) / 2);
            assert_eq!(cert.certified_liq_deficit, 0);
        }

        let close = trade(&env, -SIZE);
        let success =
            submit(&mut env, vec![close]).expect("same-slot first position closes publicly");
        peak_cu = peak_cu.max(success.compute_units_consumed);
        check(&env, true, false, early_payout);
        let payouts = [0, 1].map(|party| withdraw(&env, party, remaining[party]));
        let success = submit(&mut env, payouts.to_vec())
            .expect("both owners recover every remaining senior atom");
        peak_cu = peak_cu.max(success.compute_units_consumed);
        check(&env, true, false, [0, 1].map(|party| deposits[party] - FEE));
    }
    assert_cu_within(
        "flat withdrawal/first-admission composition",
        peak_cu,
        400_000,
    );
    println!("INV-027 flat withdrawal/first admission: 2 custody rollbacks, 2 first opens, 4 full owner exits; peak CU={peak_cu}");
}

fn issue408_advance_market_without_target_refresh(
    env: &mut V16CuEnv,
    empty: Pubkey,
    slot: u64,
    mark: u64,
) {
    env.svm.warp_to_slot(slot);
    env.push_auth_mark_for_asset_as_admin(0, slot, mark);
    env.crank(
        empty,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(0),
        },
    );
}

fn issue408_aged_portfolios() -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, Pubkey, Pubkey) {
    let mut params = production_risk_params();
    params.maintenance_fee_per_slot = ISSUE408_FEE_PER_SLOT;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let aged_owner = Keypair::new();
    let aged = env.create_portfolio(&aged_owner);
    let dust_owner = Keypair::new();
    let dust = env.create_portfolio(&dust_owner);
    let empty_owner = Keypair::new();
    let empty = env.create_portfolio(&empty_owner);
    env.deposit(&aged_owner, aged, 1_100_000);
    env.deposit(&dust_owner, dust, 600);
    env.trade_with_cu(&aged_owner, aged, &dust_owner, dust, 1, 1_000_000, 0);
    for slot in (20..=ISSUE408_AGED_SLOT).step_by(20) {
        issue408_advance_market_without_target_refresh(&mut env, empty, slot, 1_000_000);
    }

    let fresh_owner = Keypair::new();
    let fresh = env.create_portfolio(&fresh_owner);
    env.deposit(&fresh_owner, fresh, 600_000);
    (env, aged_owner, fresh_owner, aged, dust, empty, fresh)
}

#[test]
fn v16_program_issue408_unsigned_matcher_cannot_spend_aged_maintenance_collateral() {
    let (mut env, lp_owner, taker_owner, lp, dust, empty, taker) = issue408_aged_portfolios();
    assert_eq!(env.portfolio_state(lp).last_fee_slot.get(), 0);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_ctx, matcher_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &lp_owner, lp);

    // The LP owner does not sign. A public refresh followed by the taker's matcher fill must
    // crystallize the old obligation before the fill can move any of that collateral.
    env.crank(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: ISSUE408_AGED_SLOT,
            observations: crank_observations(0),
        },
    );
    env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        ISSUE408_LOTS * POS_SCALE as i128,
        0,
    );

    let lp_after_fill = env.portfolio_state(lp);
    assert_eq!(
        lp_after_fill.last_fee_slot.get(),
        ISSUE408_AGED_SLOT,
        "the unsigned LP fill must first advance the aged fee cursor"
    );
    assert_eq!(
        lp_after_fill.capital.get(),
        600_000,
        "exactly the 500-slot maintenance obligation must leave LP capital before transfer"
    );
    assert_eq!(
        env.market_state().1.insurance,
        ISSUE408_FEE_PER_SLOT * ISSUE408_AGED_SLOT as u128,
        "the crystallized obligation must be durably attributed to insurance"
    );

    issue408_advance_market_without_target_refresh(&mut env, empty, 520, 1_048_000);
    issue408_advance_market_without_target_refresh(&mut env, empty, 540, 1_098_304);
    issue408_advance_market_without_target_refresh(&mut env, empty, ISSUE408_MOVE_SLOT, 1_100_000);
    for portfolio in [lp, taker, dust] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: ISSUE408_MOVE_SLOT,
                observations: crank_observations(0),
            },
        );
    }
    assert!(
        env.market_state().1.insurance >= ISSUE408_FEE_PER_SLOT * ISSUE408_AGED_SLOT as u128,
        "later settlement cannot claw the already crystallized senior fee back out"
    );
}

#[test]
fn v16_program_issue408_liquidation_reward_cannot_preempt_aged_maintenance_collateral() {
    const FEE_PER_SLOT: u128 = 35;
    const AGED_SLOT: u64 = 4_000;
    const MOVE_SLOT: u64 = 4_020;

    let mut params = production_risk_params();
    params.maintenance_fee_per_slot = FEE_PER_SLOT;
    params.max_abs_funding_e9_per_slot = 0;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.update_liquidation_fee_policy_with_cu(10_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let empty_owner = Keypair::new();
    let empty = env.create_portfolio(&empty_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&victim_owner, victim, 600_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &victim_owner,
        victim,
        ISSUE408_LOTS * POS_SCALE as i128,
        1_000_000,
        0,
    );
    for slot in (20..=AGED_SLOT).step_by(20) {
        issue408_advance_market_without_target_refresh(&mut env, empty, slot, 1_000_000);
    }
    assert_eq!(env.portfolio_state(victim).last_fee_slot.get(), 0);

    issue408_advance_market_without_target_refresh(&mut env, empty, MOVE_SLOT, 1_048_000);
    let cranker_owner = Keypair::new();
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&cranker_owner, cranker, 1);
    let cranker_before = env.portfolio_state(cranker).capital.get();
    let insurance_before_collection = env.market_state().1.insurance;
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: MOVE_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(cranker, false),
        ],
        &[&cranker_owner],
    )
    .expect("the value-moving crank must first crystallize maintenance");

    let after_collection = env.portfolio_state(victim);
    assert_eq!(after_collection.last_fee_slot.get(), MOVE_SLOT);
    assert_eq!(after_collection.capital.get(), 0);
    assert_eq!(
        env.market_state().1.insurance - insurance_before_collection,
        120_000,
        "all available victim capital must be attributed to the older fee before any reward"
    );
    assert_eq!(
        env.portfolio_state(cranker).capital.get(),
        cranker_before,
        "the cranker cannot receive collateral already consumed by the senior fee"
    );
    let position_before_liquidation = active_leg_for_asset(&after_collection, 0)
        .basis_pos_q
        .unsigned_abs();

    // Collection invalidates the old health certificate. A subsequent public call must still
    // advance liquidation or terminal recovery; the ordering fix cannot strand the position.
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: MOVE_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(cranker, false),
        ],
        &[&cranker_owner],
    )
    .expect("fee collection must preserve a public liquidation or recovery continuation");
    assert_eq!(env.portfolio_state(cranker).capital.get(), cranker_before);
    let victim_after_liquidation = env.portfolio_state(victim);
    let position_reduced = !has_active_leg_for_asset(&victim_after_liquidation, 0)
        || active_leg_for_asset(&victim_after_liquidation, 0)
            .basis_pos_q
            .unsigned_abs()
            < position_before_liquidation;
    assert!(
        position_reduced || env.market_state().1.mode != MarketModeV16::Live,
        "the bounded follow-up crank must reduce exposure or enter terminal recovery"
    );
}

#[test]
fn v16_bpf_public_crystallized_loss_budget_credits_only_fresh_lp_principal() {
    const OPEN_PRICE: u64 = 1_000;
    const ADVERSE_PRICE: u64 = 900;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);

    let trader_owner = Keypair::new();
    let trader = env.create_portfolio(&trader_owner);
    let first_lp_owner = Keypair::new();
    let first_lp = env.create_portfolio(&first_lp_owner);
    let fresh_lp_owner = Keypair::new();
    let fresh_lp = env.create_portfolio(&fresh_lp_owner);
    env.deposit(&trader_owner, trader, 10_000);
    env.deposit(&first_lp_owner, first_lp, 10_000);
    env.deposit(&fresh_lp_owner, fresh_lp, 10_000);

    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &first_lp_owner,
        first_lp,
        POS_SCALE as i128,
        OPEN_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, ADVERSE_PRICE);
    for portfolio in [trader, first_lp] {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("authenticated adverse mark refresh");
    }

    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &first_lp_owner,
        first_lp,
        -(POS_SCALE as i128),
        ADVERSE_PRICE,
        0,
    );
    let after_loss = env.portfolio_state(trader);
    let crystallized = after_loss.residual_crystallized_loss_atoms_total.get();
    assert!(
        crystallized > 0,
        "closing the adverse position must crystallize real principal loss"
    );
    assert_eq!(
        after_loss.residual_spent_principal_atoms_total.get(),
        0,
        "closing risk creates but does not spend the residual reward budget"
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&after_loss)),
        "the losing episode is fully closed before the reward-bearing trade"
    );

    let fresh_lp_before = env.portfolio_state(fresh_lp);
    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &fresh_lp_owner,
        fresh_lp,
        POS_SCALE as i128,
        ADVERSE_PRICE,
        0,
    );

    let trader_after = env.portfolio_state(trader);
    let fresh_lp_after = env.portfolio_state(fresh_lp);
    let spent = trader_after.residual_spent_principal_atoms_total.get();
    let received = fresh_lp_after
        .residual_received_atoms_total
        .get()
        .checked_sub(fresh_lp_before.residual_received_atoms_total.get())
        .expect("monotonic recipient counter");
    assert!(
        spent > 0,
        "fresh risk consumes a nonzero real-principal budget"
    );
    assert_eq!(
        received, spent,
        "the independent LP receives exactly the trader's consumed budget"
    );
    assert!(
        spent <= crystallized,
        "public trading cannot spend more than the actual crystallized loss"
    );
    assert_eq!(
        trader_after.residual_crystallized_loss_atoms_total.get(),
        crystallized,
        "spending a reward budget never manufactures another crystallized loss"
    );
}

// security.md sweep — debtor escape / LoF for winner (#22/#48): an insolvent loser must NOT be
// able to withdraw or otherwise extract value before/at liquidation, which would strand the
// winner's claim. Probe: drive short underwater, then short attempts withdraw -> must reject; the
// winner's position and the vault backing remain intact.
#[test]
fn v16_attack_insolvent_loser_cannot_withdraw_to_escape() {
    let mut env = V16CuEnv::new();
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
                AccountMeta::new(short_account, false),
            ],
            &[],
        );
    }
    let vault_before = env.market_state().1.vault;
    // insolvent short tries to withdraw ANY amount -> must reject (margin / no free capital).
    for amt in [1u128, 100, 250] {
        env.svm.expire_blockhash();
        let dest = Pubkey::new_unique();
        env.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(env.mint, short_owner.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let r = env.send(
            env.withdraw_ix(short_account, amt),
            vec![
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_account, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&short_owner],
        );
        assert!(
            r.is_err(),
            "insolvent short must not withdraw {} to escape its debt",
            amt
        );
        let got = {
            let d = env.svm.get_account(&dest).unwrap().data;
            u64::from_le_bytes(d[64..72].try_into().unwrap())
        };
        assert_eq!(got, 0, "no tokens leaked to escaping debtor");
    }
    assert_eq!(
        env.market_state().1.vault,
        vault_before,
        "vault untouched by rejected escape attempts"
    );
}

// security.md sweep — withdraw vs open-position margin (#19/#46): an account with an open position
// must not be able to withdraw into under-collateralization (margin is conservatively reserved), yet
// its capital must remain fully recoverable once the position is closed (no permanent lock / LoF).
#[test]
fn v16_attack_withdraw_respects_margin_and_recoverable() {
    let mut env = V16CuEnv::new(); // IM = 100%, max_price_move = 100%/slot (conservative envelope)
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000);
    env.deposit(&lb, pb, 10_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, (6 * POS_SCALE) as i128, 100, 0); // notional 600

    // with the position open, withdrawing the FULL capital must reject (margin reserved).
    let try_wd = |env: &mut V16CuEnv, amt: u128| -> bool {
        env.svm.expire_blockhash();
        let d = Pubkey::new_unique();
        env.svm
            .set_account(
                d,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(env.mint, la.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.send(
            env.withdraw_ix(pa, amt),
            vec![
                AccountMeta::new(la.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(d, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&la],
        )
        .is_ok()
    };
    assert!(
        !try_wd(&mut env, 1_000),
        "cannot withdraw full capital with a position open"
    );
    assert!(
        !try_wd(&mut env, 500),
        "conservative margin reserves capital under the worst-case envelope"
    );
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        1_000,
        "capital intact after rejected withdraws (no partial debit)"
    );

    // close the position; capital must then be fully recoverable (no permanent lock / LoF).
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(6 * POS_SCALE as i128), 100, 0);
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(pa))),
        "la flat after close"
    );
    let cap = env.portfolio_state(pa).capital.get();
    env.svm.expire_blockhash();
    let (dest, _) = env.withdraw_with_cu(&la, pa, cap);
    assert_eq!(
        env.token_amount(dest) as u128,
        cap,
        "full capital recovered after closing (no LoF)"
    );
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        0,
        "capital fully withdrawn"
    );
    let (_, g) = env.market_state();
    assert_eq!(g.vault, g.c_tot + g.insurance, "conservation");
}

// security.md sweep — third-party withdraw vs winner's pnl backing (#33/#22): a winner's parked pnl is
// backed by residual (vault - c_tot - insurance). An UNRELATED account withdrawing its own capital
// reduces vault and c_tot equally, so the residual — and thus the winner's backing — must be unchanged.
#[test]
fn v16_attack_third_party_withdraw_preserves_pnl_backing() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo = Keypair::new();
    let plo = env.create_portfolio(&lo);
    let sh = Keypair::new();
    let psh = env.create_portfolio(&sh);
    let c = Keypair::new();
    let pc = env.create_portfolio(&c);
    env.deposit(&lo, plo, 1_000_000);
    env.deposit(&sh, psh, 1_000_000);
    env.deposit(&c, pc, 1_000_000); // unrelated, no position
    env.trade_asset_with_cu(0, &lo, plo, &sh, psh, (10_000 * POS_SCALE) as i128, 100, 0);
    // price up -> long parks pnl, short realizes loss (freeing residual).
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    env.crank_steps_after_market_catchup(
        psh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.crank(
        plo,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(11);
    for p in [psh, plo] {
        env.crank_if_actionable(
            p,
            ProgInstruction::PermissionlessCrank {
                now_slot: 11,
                observations: crank_observations(0),
            },
        );
    }
    let long_pnl = env.portfolio_state(plo).pnl.get();
    assert!(long_pnl > 0, "long has parked pnl (non-vacuous)");
    let (_, g0) = env.market_state();
    let resid0 = g0.vault as i128 - g0.c_tot as i128 - g0.insurance as i128;
    assert!(resid0 >= long_pnl, "long's pnl is backed by residual");

    // unrelated account C withdraws ALL its capital.
    env.svm.expire_blockhash();
    let (_d, _) = env.withdraw_with_cu(&c, pc, 1_000_000);
    let (_, g1) = env.market_state();
    let resid1 = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    // residual (and thus the winner's backing) is UNCHANGED by C's withdrawal.
    assert_eq!(
        resid1, resid0,
        "third-party withdraw did NOT change the residual backing the winner"
    );
    assert!(
        resid1 >= env.portfolio_state(plo).pnl.get().max(0),
        "long's pnl still fully backed"
    );
    assert_eq!(
        g1.vault,
        g0.vault - 1_000_000,
        "vault decreased by exactly C's withdrawal"
    );
    assert_eq!(
        g1.c_tot,
        g0.c_tot - 1_000_000,
        "c_tot decreased by exactly C's withdrawal"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — deposit does not dilute an existing junior-PnL holder's backing (#33/#22 interaction):
// a backed junior-PnL holder's realizable claim is funded by residual (vault - c_tot - insurance). A
// fresh deposit by ANOTHER account increases vault AND c_tot equally, so residual — and the holder's
// haircut-backed equity — must be UNCHANGED. Attacker/edge goal: a large third-party deposit shifts the
// backing math so the holder's realizable claim grows (mint) or shrinks (theft). Protection: residual is
// invariant to deposits, so the holder's certified equity is identical before/after.
#[test]
fn v16_attack_deposit_does_not_dilute_junior_backing() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000); // backing for the junior holder
    let ho = Keypair::new();
    let h = env.create_portfolio(&ho); // junior-pnl holder
    env.deposit(&ho, h, 1_000);
    env.add_source_positive_pnl(h, 1, 40); // 40 backed positive pnl
    env.crank(
        h,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let h_pre = env.portfolio_state(h);
    let g_pre = env.market_state().1;
    let residual_pre = g_pre
        .vault
        .saturating_sub(g_pre.c_tot)
        .saturating_sub(g_pre.insurance);
    assert!(health_cert(&h_pre).valid, "holder cert valid");
    assert!(
        h_pre.pnl.get() > 0,
        "holder carries positive (backed) junior pnl, pnl={}",
        h_pre.pnl.get()
    );
    let eq_pre = health_cert(&h_pre).certified_equity;

    // a DIFFERENT account makes a large deposit.
    let wo = Keypair::new();
    let w = env.create_portfolio(&wo);
    env.deposit(&wo, w, 5_000_000);

    // refresh the holder's cert (deposit by w should not have changed the holder's backing).
    env.svm.expire_blockhash();
    assert!(
        env.crank_if_actionable(
            h,
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
        )
        .is_none(),
        "an unrelated deposit must not make the holder actionable"
    );
    let h_post = env.portfolio_state(h);
    let g_post = env.market_state().1;
    let residual_post = g_post
        .vault
        .saturating_sub(g_post.c_tot)
        .saturating_sub(g_post.insurance);

    // NON-DILUTION: residual is invariant to the third-party deposit (vault += D, c_tot += D).
    assert_eq!(
        residual_post, residual_pre,
        "residual unchanged by a third-party deposit (no dilution/mint)"
    );
    // the holder's realizable claim (capital + backed pnl) and pnl are byte-identical.
    assert_eq!(
        health_cert(&h_post).certified_equity,
        eq_pre,
        "holder certified equity unchanged by w's deposit"
    );
    assert_eq!(
        h_post.capital.get(),
        h_pre.capital.get(),
        "holder capital unchanged"
    );
    assert_eq!(
        h_post.pnl.get(),
        h_pre.pnl.get(),
        "holder junior pnl unchanged"
    );
    // conservation, and the deposit landed as real tokens.
    assert_eq!(
        g_post.vault,
        g_pre.vault + 5_000_000,
        "vault grew by exactly the deposit"
    );
    assert_eq!(
        g_post.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation"
    );
}

// security.md sweep — insurance-fund ops do not touch junior-PnL backing (#6/#33/#22 interaction): a
// backed junior holder is funded by residual (vault − c_tot − insurance). A domain insurance top-up
// (+X to vault AND insurance) and a domain insurance withdrawal (−X to vault AND insurance) both leave
// residual invariant. Attacker/edge goal: route value through the insurance fund to shift a junior
// holder's realizable claim (mint by growing it / theft by shrinking it). Protection: residual — and the
// holder's certified equity — is identical across both insurance operations; senior conservation holds.
#[test]
fn v16_attack_insurance_ops_preserve_junior_backing() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000);
    let ho = Keypair::new();
    let h = env.create_portfolio(&ho);
    env.deposit(&ho, h, 1_000);
    env.add_source_positive_pnl(h, 1, 40);
    env.crank(
        h,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let eq0 = health_cert(&env.portfolio_state(h)).certified_equity;
    let g0 = env.market_state().1;
    let residual0 = g0
        .vault
        .saturating_sub(g0.c_tot)
        .saturating_sub(g0.insurance);
    assert!(
        env.portfolio_state(h).pnl.get() > 0,
        "holder carries backed junior pnl"
    );

    let read = |env: &V16CuEnv| -> (i128, u128) {
        let g = env.market_state().1;
        (
            g.vault.saturating_sub(g.c_tot).saturating_sub(g.insurance) as i128,
            g.insurance,
        )
    };

    // (1) domain insurance TOP-UP: +1M to vault AND insurance -> residual unchanged.
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 0, 1_000_000);
    let (res1, ins1) = read(&env);
    assert_eq!(
        res1, residual0 as i128,
        "residual unchanged by insurance top-up"
    );
    assert_eq!(
        ins1,
        g0.insurance + 1_000_000,
        "insurance grew by the top-up"
    );

    // refresh the holder's cert: top-up must not have changed its backing.
    env.svm.expire_blockhash();
    env.crank_if_actionable(
        h,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    assert_eq!(
        health_cert(&env.portfolio_state(h)).certified_equity,
        eq0,
        "holder equity unchanged by insurance top-up"
    );

    // (2) domain insurance WITHDRAW: −600k from vault AND insurance -> residual STILL unchanged.
    env.try_withdraw_insurance_domain_with_authority(&admin, 0, 600_000)
        .expect("domain withdraw ok");
    let (res2, ins2) = read(&env);
    assert_eq!(
        res2, residual0 as i128,
        "residual unchanged by insurance withdrawal"
    );
    assert_eq!(ins2, ins1 - 600_000, "insurance dropped by the withdrawal");

    // refresh the holder's cert: withdrawal must not have touched its backing either.
    env.svm.expire_blockhash();
    env.crank_if_actionable(
        h,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let hf = env.portfolio_state(h);
    let gf = env.market_state().1;
    assert_eq!(
        health_cert(&hf).certified_equity,
        eq0,
        "holder equity unchanged across BOTH insurance ops"
    );
    assert_eq!(
        hf.pnl.get(),
        env.portfolio_state(h).pnl.get(),
        "holder pnl stable"
    );
    assert_eq!(
        gf.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        gf.vault >= gf.c_tot + gf.insurance,
        "senior conservation across insurance ops"
    );
}

// security.md sweep — liquidation fee is bounded by liquidation_fee_cap (#3/#33): the liquidation fee is
// min(liquidation_fee_bps * notional, liquidation_fee_cap). Attacker/edge goal: a large liquidation
// charges an unbounded fee (bps*notional) draining the victim / over-feeding insurance+cranker past the
// security.md sweep — repeated partial-liquidation fee stop (#3/#33): the liquidation fee is charged
// only for an engine-selected liquidation. Attacker/cranker goal: keep resubmitting the same partial
// close hint after the first liquidation restored health, charging the victim over and over. Protection:
// security.md sweep — leveraged bad-debt socialization (#9/#22/#33): at 20x leverage (5% margin) a
// thin-margin short's loss can EXCEED its capital, creating bad debt. Attacker goal: the loser's
// unrecovered deficit is printed as the winner's spendable gain (vault mint / senior over-pay).
// Protection: the loser's capital floors at 0 (loss capped at capital), the unrecovered deficit is
// absorbed as un-backed junior pnl on the winner side (not minted), and the vault is never inflated.
#[test]
fn v16_attack_leveraged_bad_debt_socialized_not_printed() {
    const SHORT_CAP: u128 = 55_000; // ~10% above the 50_000 (5% of 1e6) initial margin
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, SHORT_CAP);
    let total_deposits = 100_000_000u128 + SHORT_CAP;
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    assert_eq!(
        env.market_state().1.vault,
        total_deposits,
        "vault == total deposits at open"
    );

    // drive the mark to 1.07e6: short loss ≈ 70_000 > its 55_000 capital -> 15_000 bad debt.
    for slot in 1..=40u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 1_070_000);
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
    // NON-VACUITY: the short is insolvent — its capital was fully consumed by the loss.
    assert_eq!(
        env.portfolio_state(s).capital.get(),
        0,
        "short capital wiped to 0 by the loss (bad debt exists)"
    );

    // The bounded public loop above already performed the liquidation/settlement work.
    let (_, g) = env.market_state();

    // NO MINT: the unrecovered bad debt is NOT printed — vault stays == total deposits the whole way.
    assert_eq!(
        g.vault, total_deposits,
        "vault never inflated by the bad debt (no mint)"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    // short capital still 0 (never negative); senior conservation preserved through the bad-debt event.
    assert_eq!(
        env.portfolio_state(s).capital.get(),
        0,
        "short capital floored at 0 (never negative)"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under leveraged bad debt"
    );
    // the winner's gain that exceeds the recoverable backing is un-backed junior pnl (residual-bounded),
    // not spendable senior capital -> the bad debt is socialized, not paid out.
    let lw = env.portfolio_state(l);
    let residual = g.vault.saturating_sub(g.c_tot).saturating_sub(g.insurance);
    assert!(
        (health_cert(&lw).certified_equity as u128) <= lw.capital.get() + residual + 1,
        "winner realizable bounded by capital+residual (bad debt not realizable)"
    );
}

fn inv027_try_withdraw_backing(
    env: &mut V16CuEnv,
    domain: u16,
    ledger: Option<Pubkey>,
    amount: u128,
) -> (Pubkey, Result<u64, String>) {
    let authority = env.admin.insecure_clone();
    let destination = env.token_account(authority.pubkey(), 0);
    let market_id = env.asset_market_id(domain / 2);
    let authority_epoch =
        env.withdrawal_authority_epoch(authority.pubkey(), domain as usize / 2, false);
    let instruction = if ledger.is_some() {
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain,
            market_id,
            authority_epoch,
            amount,
        }
    } else {
        ProgInstruction::WithdrawBackingBucket {
            domain,
            market_id,
            authority_epoch,
            amount,
        }
    };
    let mut accounts = vec![
        AccountMeta::new(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    if let Some(ledger) = ledger {
        accounts.push(AccountMeta::new(ledger, false));
    }
    accounts.extend([
        AccountMeta::new(destination, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]);
    let result = env.send(instruction, accounts, &[&authority]);
    (destination, result)
}

fn inv027_try_withdraw_insurance(
    env: &mut V16CuEnv,
    asset_index: u16,
    amount: u128,
) -> (Pubkey, Result<u64, String>) {
    let authority = env.admin.insecure_clone();
    let destination = env.token_account(authority.pubkey(), 0);
    let market_id = env.asset_market_id(asset_index);
    let authority_epoch =
        env.withdrawal_authority_epoch(authority.pubkey(), asset_index as usize, true);
    let result = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index,
            market_id,
            authority_epoch,
            amount,
        },
        vec![
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&authority],
    );
    (destination, result)
}

#[test]
fn v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit() {
    const WITHDRAW_AMOUNT: u128 = 1;

    // This fixture earns provider fees through real deposits, matched trades, authenticated marks,
    // and a signed backing-fee cap. No program-owned byte is injected by this test.
    let fixture = public_backing_earnings_fixture();
    let mut env = fixture.env;
    let domain = fixture.domain;
    let asset_index = domain / 2;
    let ledger = fixture.ledger;
    assert!(fixture.earnings >= WITHDRAW_AMOUNT);
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, asset_index * 2, 100);

    let before_stale = env.market_state().1;
    assert!(
        before_stale.source_backing_buckets[domain as usize].fresh_unliened_backing_num
            >= WITHDRAW_AMOUNT,
        "the stale withdrawal probe needs real unencumbered provider principal"
    );
    assert!(
        before_stale.source_backing_buckets[domain as usize].utilization_fee_earnings
            >= WITHDRAW_AMOUNT,
        "the stale withdrawal probe needs publicly earned provider fees"
    );
    assert!(
        before_stale.insurance_domain_budget[(asset_index as usize) * 2] >= WITHDRAW_AMOUNT,
        "the stale withdrawal probe needs publicly deposited insurance"
    );

    // Advance asset 1 only. Asset 0 remains locally loss-stale with live exposure, so reserve
    // withdrawals from asset 0 must remain locked even if the market's last-touched cache changes.
    let cranker_owner = Keypair::new();
    let cranker = env.create_portfolio(&cranker_owner);
    env.svm.warp_to_slot(3);
    env.crank(
        cranker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(1),
        },
    );
    let stale = env.market_state().1;
    assert_eq!(stale.current_slot, 3);
    assert!(stale.assets[asset_index as usize].slot_last < stale.current_slot);
    assert!(
        stale.assets[asset_index as usize].oi_eff_long_q != 0
            || stale.assets[asset_index as usize].oi_eff_short_q != 0,
        "asset-local stale state must protect a live economic obligation"
    );

    let assert_rejected_without_stock_mutation =
        |env: &V16CuEnv, market_before: &Account, vault_before: &Account, label: &str| {
            assert_eq!(
                env.svm.get_account(&env.market).as_ref(),
                Some(market_before),
                "{label}: rejected withdrawal changed market state"
            );
            assert_eq!(
                env.svm.get_account(&env.vault).as_ref(),
                Some(vault_before),
                "{label}: rejected withdrawal changed SPL custody"
            );
        };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (principal_destination, principal_result) =
        inv027_try_withdraw_backing(&mut env, domain, None, WITHDRAW_AMOUNT);
    assert!(
        principal_result.is_err(),
        "loss-stale backing principal escaped"
    );
    assert_rejected_without_stock_mutation(
        &env,
        &market_before,
        &vault_before,
        "backing principal",
    );
    assert_eq!(env.token_amount(principal_destination), 0);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let (earnings_destination, earnings_result) =
        inv027_try_withdraw_backing(&mut env, domain, Some(ledger), WITHDRAW_AMOUNT);
    assert!(
        earnings_result.is_err(),
        "loss-stale provider earnings escaped"
    );
    assert_rejected_without_stock_mutation(
        &env,
        &market_before,
        &vault_before,
        "provider earnings",
    );
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.token_amount(earnings_destination), 0);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (insurance_destination, insurance_result) =
        inv027_try_withdraw_insurance(&mut env, asset_index, WITHDRAW_AMOUNT);
    assert!(insurance_result.is_err(), "loss-stale insurance escaped");
    assert_rejected_without_stock_mutation(&env, &market_before, &vault_before, "insurance");
    assert_eq!(env.token_amount(insurance_destination), 0);

    // The reserve gate must not become a market-wide user lock. A new flat user can enter, pay the
    // authenticated maintenance debt dictated by this fixture, and withdraw every remaining atom.
    let flat_owner = Keypair::new();
    let flat = env.create_portfolio(&flat_owner);
    env.deposit(&flat_owner, flat, 10_000);
    env.sync_maintenance_fee_with_cu(flat, None, 3);
    let withdrawable = env.portfolio_state(flat).capital.get();
    assert!(
        withdrawable > 0,
        "maintenance settlement must leave a nonvacuous exit"
    );
    let destination = env.withdraw(&flat_owner, flat, withdrawable);
    assert_eq!(
        u128::from(env.token_amount(destination)),
        withdrawable,
        "unrelated flat senior capital must remain fully withdrawable after its fee"
    );
    assert_eq!(env.portfolio_state(flat).capital.get(), 0);
    let final_group = env.market_state().1;
    assert!(final_group.assets[asset_index as usize].slot_last < final_group.current_slot);
    assert_eq!(final_group.vault as u64, env.token_amount(env.vault));
    assert!(final_group.vault >= final_group.c_tot + final_group.insurance);
}

#[test]
fn v16_program_stale_positive_pnl_cannot_fund_withdrawal_ahead_of_maintenance() {
    const PRINCIPAL: u128 = 1_000;
    const FEE_PER_SLOT: u128 = 7;
    const PROFIT: u128 = 50;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        maintenance_fee_per_slot: FEE_PER_SLOT,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.top_up_backing_bucket(1, 75, 100);
    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, PRINCIPAL);
    env.deposit(&loser_owner, loser, PRINCIPAL);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        SIZE_Q,
        100,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    for portfolio in [loser, winner] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        -SIZE_Q,
        105,
        0,
    );
    let released = env.portfolio_state(winner);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&released)));
    assert!(health_cert(&released).valid);
    assert_eq!(
        health_cert(&released).cert_oracle_epoch,
        env.market_state().1.oracle_epoch
    );
    assert_eq!(released.capital.get(), PRINCIPAL - FEE_PER_SLOT);
    assert_eq!(released.pnl.get(), PROFIT as i128);
    assert_eq!(released.last_fee_slot.get(), 2);
    assert_eq!(
        env.portfolio_state(loser).capital.get(),
        PRINCIPAL - FEE_PER_SLOT - PROFIT
    );

    let keeper_owner = Keypair::new();
    let keeper = env.create_portfolio(&keeper_owner);
    env.svm.warp_to_slot(3);
    env.push_auth_mark_for_asset_as_admin(0, 3, 106);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let stale = env.portfolio_state(winner);
    let before = env.market_state().1;
    assert_eq!(health_cert(&stale), health_cert(&released));
    assert!(health_cert(&stale).cert_oracle_epoch < before.oracle_epoch);
    assert_eq!(stale.capital.get(), released.capital.get());
    assert_eq!(stale.last_fee_slot.get(), 2);
    assert_eq!(stale.pnl.get(), PROFIT as i128);
    let capital = PRINCIPAL - FEE_PER_SLOT;
    let post_fee_principal = capital - FEE_PER_SLOT;
    assert!(post_fee_principal + 1 < capital);
    assert!(
        PROFIT > FEE_PER_SLOT,
        "junior value could mask the entire unpaid fee"
    );

    let destination = env.token_account(winner_owner.pubkey(), 0);
    let withdraw_accounts = vec![
        AccountMeta::new(winner_owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(winner, false),
        AccountMeta::new(destination, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    let tracked = [
        env.market,
        winner,
        loser,
        keeper,
        env.vault,
        destination,
        env.mint,
        winner_owner.pubkey(),
        env.vault_authority,
        spl_token::ID,
    ];
    let reject_exactly = |env: &mut V16CuEnv, ix, accounts| {
        let snapshot: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
        let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        env.svm.expire_blockhash();
        let error = env
            .send(ix, accounts, &[&winner_owner])
            .expect_err("junior value is not spendable ahead of the fee");
        assert!(error.contains("InstructionError(2, Custom("), "{error}");
        for (key, account) in tracked.iter().zip(snapshot) {
            assert!(
                env.svm.get_account(key) == account,
                "rejected instruction changed {key}"
            );
        }
        payer.lamports -= 2 * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
        assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    };

    // Unlike the isolated stale-conversion control, this withdrawal stages a real fee debit
    // before failing one atom above post-fee principal, despite sufficient positive PnL.
    for _ in 0..2 {
        let ix = env.withdraw_ix(winner, post_fee_principal + 1);
        reject_exactly(&mut env, ix, withdraw_accounts.clone());
        let ix = env.convert_released_pnl_ix(winner, PROFIT);
        let accounts = vec![
            AccountMeta::new(winner_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
        ];
        reject_exactly(&mut env, ix, accounts);
    }

    let protected_loser = env.svm.get_account(&loser).unwrap();
    env.svm.expire_blockhash();
    env.send(
        env.withdraw_ix(winner, capital),
        withdraw_accounts.clone(),
        &[&winner_owner],
    )
    .expect("withdraw-all settles the fee and returns only remaining principal");
    let exited = env.portfolio_state(winner);
    let after_exit = env.market_state().1;
    assert_eq!(exited.capital.get(), 0);
    assert_eq!(exited.pnl.get(), PROFIT as i128);
    assert_eq!(exited.last_fee_slot.get(), 3);
    assert_eq!(
        u128::from(env.token_amount(destination)),
        post_fee_principal
    );
    assert_eq!(after_exit.insurance, before.insurance + FEE_PER_SLOT);
    assert_eq!(after_exit.c_tot, before.c_tot - capital);
    assert_eq!(after_exit.vault, before.vault - post_fee_principal);
    assert_eq!(after_exit.source_credit[1], before.source_credit[1]);
    assert_eq!(
        after_exit.source_backing_buckets[1],
        before.source_backing_buckets[1]
    );

    env.svm.expire_blockhash();
    env.crank(
        winner,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let current = env.portfolio_state(winner);
    let before_conversion = env.market_state().1;
    assert!(health_cert(&current).valid);
    assert_eq!(
        health_cert(&current).cert_oracle_epoch,
        before_conversion.oracle_epoch
    );
    env.convert_released_pnl_with_cu(&winner_owner, winner, PROFIT);
    let converted = env.portfolio_state(winner);
    let after_conversion = env.market_state().1;
    assert_eq!(converted.capital.get(), PROFIT);
    assert_eq!(converted.pnl.get(), 0);
    assert_eq!(after_conversion.c_tot, after_exit.c_tot + PROFIT);
    assert_eq!(after_conversion.insurance, after_exit.insurance);
    assert_eq!(after_conversion.vault, after_exit.vault);
    assert_eq!(
        after_conversion.source_backing_buckets[1].consumed_liened_backing_num,
        before_conversion.source_backing_buckets[1].consumed_liened_backing_num
            + PROFIT * BOUND_SCALE,
    );
    assert_eq!(
        after_conversion.source_credit[1].positive_claim_bound_num + PROFIT * BOUND_SCALE,
        before_conversion.source_credit[1].positive_claim_bound_num,
    );
    env.svm.expire_blockhash();
    env.send(
        env.withdraw_ix(winner, PROFIT),
        withdraw_accounts,
        &[&winner_owner],
    )
    .expect("current converted claim retains its exact public payout");
    assert_eq!(env.portfolio_state(winner).capital.get(), 0);
    assert_eq!(
        u128::from(env.token_amount(destination)),
        post_fee_principal + PROFIT
    );
    assert_eq!(env.svm.get_account(&loser).unwrap(), protected_loser);
    let final_group = env.market_state().1;
    assert_eq!(final_group.insurance, before.insurance + FEE_PER_SLOT);
    assert_eq!(final_group.c_tot, before.c_tot - capital);
    assert_eq!(
        final_group.vault,
        before.vault - post_fee_principal - PROFIT
    );
    assert_eq!(final_group.vault, u128::from(env.token_amount(env.vault)));
    assert!(final_group.vault >= final_group.c_tot + final_group.insurance);
}

#[derive(Clone, Copy)]
struct Inv027LossStaleRoute {
    owner: &'static str,
    marker: &'static str,
    count: usize,
    disposition: &'static str,
    witnesses: &'static [&'static str],
}

#[test]
fn v16_program_loss_stale_economic_routes_have_a_complete_seniority_disposition() {
    const ENGINE_PIN: &str = "394fd0bf2cb7d73df425eb3754dc3be1a0c44336";
    const ROWS: &[Inv027LossStaleRoute] = &[
        Inv027LossStaleRoute {
            owner: "handle_deposit",
            marker: ".deposit_not_atomic(",
            count: 1,
            disposition: "external-value-in remains available",
            witnesses: &[
                "v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_withdraw",
            marker: ".withdraw_not_atomic(",
            count: 1,
            disposition: "flat senior-capital exit remains available",
            witnesses: &[
                "v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_trade_nocpi_zero_copy",
            marker: ".execute_trade_with_fee_loss_stale_scoped_not_atomic(",
            count: 2,
            disposition: "affected risk increase rejects; canonical reduction remains live",
            witnesses: &[
                "v16_program_stale_cohort_route_matrix_preserves_historical_principal",
                "v16_bpf_stale_asset_does_not_block_current_unrelated_trade",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_batch_execute_zero_copy",
            marker: ".execute_batch_with_fee_loss_stale_scoped_not_atomic(",
            count: 1,
            disposition: "batch applies the same affected-versus-unrelated scope rule",
            witnesses: &["v16_program_stale_cohort_route_matrix_preserves_historical_principal"],
        },
        Inv027LossStaleRoute {
            owner: "handle_convert_released_pnl",
            marker: ".convert_released_pnl_to_capital_not_atomic(",
            count: 1,
            disposition: "junior-to-senior conversion requires current complete certification",
            witnesses: &[
                "v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_rebalance_reduce",
            marker: ".rebalance_reduce_position_not_atomic(",
            count: 1,
            disposition: "owner risk reduction settles stale obligations before detaching exposure",
            witnesses: &["v16_program_stale_cohort_route_matrix_preserves_historical_principal"],
        },
        Inv027LossStaleRoute {
            owner: "handle_withdraw_backing_bucket",
            marker: ".withdraw_fresh_counterparty_backing_not_atomic(",
            count: 1,
            disposition: "live loss state locks provider principal",
            witnesses: &[
                "v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_withdraw_backing_bucket_earnings",
            marker: ".withdraw_backing_provider_earnings_not_atomic(",
            count: 1,
            disposition: "live loss state locks provider earnings",
            witnesses: &[
                "v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit",
            ],
        },
        Inv027LossStaleRoute {
            owner: "debit_market_insurance_budget_view",
            marker: ".withdraw_domain_insurance_not_atomic(",
            count: 2,
            disposition: "live loss state locks both side-local insurance budgets",
            witnesses: &[
                "v16_program_loss_stale_reserve_matrix_preserves_senior_stocks_and_flat_exit",
                "v16_bpf_resolved_terminal_insurance_drains_dynamic_domain_after_positions_close",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_close_resolved",
            marker: ".permissionless_auto_crank_not_atomic(",
            count: 1,
            disposition: "terminal settlement pays through the canonical priority selector",
            witnesses: &[
                "v16_program_flat_negative_final_leg_route_matrix_reaches_terminal_payout",
            ],
        },
        Inv027LossStaleRoute {
            owner: "handle_permissionless_crank_zero_copy",
            marker: ".permissionless_auto_crank_not_atomic(",
            count: 3,
            disposition: "permissionless bounded work settles stale obligations",
            witnesses: &["v16_program_multileg_loss_stale_account_has_permissionless_progress"],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-027 route composition must be reviewed on every engine pin change",
    );

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let markers: std::collections::BTreeSet<_> = ROWS.iter().map(|row| row.marker).collect();
    let mut current_function = "<module>";
    let mut actual = std::collections::BTreeMap::<(String, String), usize>::new();
    for line in production.lines() {
        let trimmed = line.trim_start();
        if let Some(fn_offset) = trimmed.find("fn ") {
            let prefix = &trimmed[..fn_offset];
            if prefix.is_empty() || prefix.starts_with("pub") {
                let rest = &trimmed[fn_offset + 3..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                current_function = &rest[..end];
            }
        }
        for marker in &markers {
            let count = line.matches(marker).count();
            if count != 0 {
                *actual
                    .entry((current_function.to_string(), (*marker).to_string()))
                    .or_default() += count;
            }
        }
    }

    let witness_sources = [
        include_str!("inv_027_protected_principal_seniority.rs"),
        include_str!("inv_054_certificate_epoch_completeness.rs"),
        include_str!("inv_064_insurance_withdrawal_policy_equivalence.rs"),
        include_str!("inv_074_scope_locality.rs"),
        include_str!("../stateful/inv_027_protected_principal_seniority.rs"),
        include_str!("../stateful/inv_071_crank_progress.rs"),
        include_str!("../stateful/inv_082_state_indexed_liveness_theorem.rs"),
    ];
    let mut expected = std::collections::BTreeMap::new();
    for row in ROWS {
        assert!(!row.disposition.is_empty());
        assert!(!row.witnesses.is_empty());
        for witness in row.witnesses {
            assert!(
                witness_sources
                    .iter()
                    .any(|source| source.contains(&format!("fn {witness}"))),
                "{}.{} lacks executable seniority witness {witness}",
                row.owner,
                row.marker,
            );
        }
        assert!(
            expected
                .insert((row.owner.to_string(), row.marker.to_string()), row.count)
                .is_none(),
            "duplicate INV-027 route classification for {}.{}",
            row.owner,
            row.marker,
        );
    }
    assert_eq!(
        actual, expected,
        "every current loss-stale economic ingress needs an explicit seniority disposition",
    );

    let mut gate_calls = std::collections::BTreeMap::<String, usize>::new();
    current_function = "<module>";
    for line in production.lines() {
        let trimmed = line.trim_start();
        if let Some(fn_offset) = trimmed.find("fn ") {
            let prefix = &trimmed[..fn_offset];
            if prefix.is_empty() || prefix.starts_with("pub") {
                let rest = &trimmed[fn_offset + 3..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                current_function = &rest[..end];
            }
        }
        if current_function != "live_domain_withdraw_health_or_shutdown_view" {
            let count = line
                .matches("live_domain_withdraw_health_or_shutdown_view(")
                .count();
            if count != 0 {
                *gate_calls.entry(current_function.to_string()).or_default() += count;
            }
        }
    }
    assert_eq!(
        gate_calls,
        std::collections::BTreeMap::from([
            ("handle_withdraw_backing_bucket".to_string(), 1),
            ("handle_withdraw_backing_bucket_earnings".to_string(), 1),
            ("handle_withdraw_insurance_asset".to_string(), 1),
        ]),
        "all live reserve withdrawals must share the exact stale-loss gate",
    );

    let transition_census =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_census.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));
}
