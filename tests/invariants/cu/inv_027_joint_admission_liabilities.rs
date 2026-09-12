//! INV-027 / reopening 413: admission must account the conjunction of uncollected
//! maintenance and rounded adverse nontraded-leg target lag, on either party.
//! INV-060 owns fees alone; INV-053 owns lag after explicit fee collection. This
//! matrix compares direct admission with public settlement while both are pending.
//! A separate close/age/reopen history covers an explicit public fee/refresh
//! prefix on previously exposed flat accounts, including rollback and exact IM.
//! The standalone sibling covers sufficiently funded first admission and deferred
//! fee disposition; admission at the uncollected-fee margin boundary remains open.
//! The funding sibling adds nonzero rounded funding debt and price-cap carry,
//! opposite-route rollback/retry, and senior principal exit ahead of a junior claim.
//! The unfunded-credit sibling separates two old funding counterparties from the
//! admission peer, enforcing gross debits before claim support and lien admission.
//! All economic state is constructed through System/SPL/ATA/wrapper instructions.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::{assert_current_certificate_matches_independent, TradeRoute};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[path = "inv_027_reward_recipient_first_risk.rs"]
mod reward_recipient_first_risk;

#[path = "inv_027_reward_mapping_admission.rs"]
mod reward_mapping_admission;

#[path = "inv_027_standalone_first_admission.rs"]
mod standalone_first_admission;

#[path = "inv_027_first_risk_preexisting_lag.rs"]
mod first_risk_preexisting_lag;

#[path = "inv_027_funding_admission.rs"]
mod funding_admission;

#[path = "inv_027_unfunded_credit_admission.rs"]
mod unfunded_credit_admission;

#[path = "inv_027_flat_reopen_routes.rs"]
mod flat_reopen_routes;

#[path = "inv_027_first_batch_fee_boundary.rs"]
mod first_batch_fee_boundary;

const PRICE: u64 = 100;
const START: u64 = 1;
const ADMISSION: u64 = 4;
const FEE_RATE: u128 = 7;
const FEE: u128 = FEE_RATE * (ADMISSION - START) as u128;
const OLD_SIZE: i128 = (10 * POS_SCALE + POS_SCALE / 10) as i128;
const NEW_SIZE: i128 = POS_SCALE as i128;
const MARGIN_BPS: u64 = 1_000;

fn requirement(size: i128) -> u128 {
    let notional = (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(MARGIN_BPS)).div_ceil(10_000)
}

fn public_portfolio(env: &mut V16CuEnv, owner: &Keypair) -> Pubkey {
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
    .expect("initialize System-created portfolio");
    env.portfolios.push(key.pubkey());
    key.pubkey()
}

fn public_deposit(env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey, amount: u128) -> Pubkey {
    let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &token,
            &env.admin.pubkey(),
            &[],
            amount.try_into().unwrap(),
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("mint collateral through SPL");
    env.send(
        env.deposit_ix(portfolio, amount),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("deposit publicly minted collateral");
    token
}

#[test]
fn v16_program_flat_reopen_fee_history_precedes_new_exposure() {
    use crate::support::fuzz_model::{
        assert_market_stock_census, assert_reservation_encumbrance_census,
    };

    const CLOSE: u64 = 2;
    const COLLECTED: u128 = FEE_RATE * (CLOSE - START) as u128;
    const PENDING: u128 = FEE_RATE * (ADMISSION - CLOSE) as u128;
    const DEPOSITS: [u128; 2] = [100 + FEE, 200 + FEE];
    const OPEN_SIZE: i128 = POS_SCALE as i128 / 2;
    const REOPEN_SIZE: i128 = POS_SCALE as i128;
    const EXCESS: i128 = REOPEN_SIZE + (POS_SCALE / PRICE as u128) as i128;

    let requirement = |size: i128| (size.unsigned_abs() * PRICE as u128).div_ceil(POS_SCALE);
    assert_eq!((COLLECTED, PENDING, FEE), (7, 14, 21));
    assert_eq!((requirement(REOPEN_SIZE), requirement(EXCESS)), (100, 101));
    assert!(DEPOSITS[0] - COLLECTED >= requirement(EXCESS));
    assert_eq!(DEPOSITS[0] - COLLECTED - PENDING, requirement(REOPEN_SIZE));

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
    env.svm.warp_to_slot(START);
    env.configure_auth_mark_for_asset_as_admin(0, START, PRICE);
    let owners = [Keypair::new(), Keypair::new()];
    let portfolios = owners
        .each_ref()
        .map(|owner| public_portfolio(&mut env, owner));
    let tokens = std::array::from_fn::<_, 2, _>(|party| {
        public_deposit(&mut env, &owners[party], portfolios[party], DEPOSITS[party])
    });
    let keeper_owner = Keypair::new();
    let keeper = public_portfolio(&mut env, &keeper_owner);
    env.trade_asset_with_cu(
        0,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        OPEN_SIZE,
        PRICE,
        0,
    );
    for portfolio in portfolios {
        let account = env.portfolio_state(portfolio);
        assert_eq!(
            active_leg_for_asset(&account, 0).basis_pos_q.unsigned_abs(),
            OPEN_SIZE as u128
        );
        assert_eq!(account.last_fee_slot.get(), START);
    }
    env.svm.warp_to_slot(CLOSE);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: CLOSE,
            observations: crank_observations(0),
        },
    );
    env.trade_asset_with_cu(
        0,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        -OPEN_SIZE,
        PRICE,
        0,
    );
    for party in 0..2 {
        let account = env.portfolio_state(portfolios[party]);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
        assert_eq!(account.capital.get(), DEPOSITS[party] - COLLECTED);
        assert_eq!(account.last_fee_slot.get(), CLOSE);
        assert_eq!(account.fee_credits.get(), 0);
        assert_eq!(account.pnl.get(), 0);
    }
    let closed = portfolios.map(|key| env.svm.get_account(&key));
    for slot in CLOSE + 1..=ADMISSION {
        env.svm.warp_to_slot(slot);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        portfolios.map(|key| env.svm.get_account(&key)),
        closed,
        "aging the market must leave both closed accounts' local fee history untouched"
    );
    let group = env.market_state().1;
    assert_eq!(group.current_slot, ADMISSION);
    assert_eq!(group.assets[0].slot_last, ADMISSION);
    assert_eq!(group.assets[0].effective_price, PRICE);
    assert_eq!(group.assets[0].raw_oracle_target_price, PRICE);
    assert_eq!(
        (
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q
        ),
        (0, 0)
    );
    assert_eq!(group.c_tot, DEPOSITS.iter().sum::<u128>() - 2 * COLLECTED);
    assert_eq!(group.insurance, 2 * COLLECTED);

    let tracked = [
        env.market,
        portfolios[0],
        portfolios[1],
        keeper,
        env.mint,
        env.vault,
        tokens[0],
        tokens[1],
        owners[0].pubkey(),
        owners[1].pubkey(),
        keeper_owner.pubkey(),
        env.admin.pubkey(),
        env.vault_authority,
        env.program_id,
        spl_token::ID,
    ];
    let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
    let before = snapshot(&env);
    let mut prefix = Vec::new();
    for portfolio in portfolios {
        prefix.push(Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: ProgInstruction::SyncMaintenanceFee {
                now_slot: ADMISSION,
            }
            .encode(),
        });
        prefix.push(Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(keeper_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: ProgInstruction::PermissionlessCrank {
                now_slot: ADMISSION,
                observations: crank_observations(0),
            }
            .encode(),
        });
    }
    let submit = |env: &mut V16CuEnv, size| {
        let mut instructions = vec![heap_ix(), cu_ix()];
        instructions.extend_from_slice(&prefix);
        instructions.push(Instruction {
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
        });
        env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&env.payer.pubkey()),
            &[&env.payer, &owners[0], &owners[1], &keeper_owner][..],
            env.svm.latest_blockhash(),
        );
        tx.verify().expect("public reopen bundle signatures");
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        env.svm.send_transaction(tx)
    };
    let failure = submit(&mut env, EXCESS).expect_err("new exposure must fit post-fee equity");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            6,
            InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)
        ),
        "the fee/refresh prefix must succeed before margin rejects: {failure:?}"
    );
    assert_eq!(
        snapshot(&env),
        before,
        "rejected reopen must restore the closed episode, fee cursors, insurance and custody"
    );

    let admitted = submit(&mut env, REOPEN_SIZE).expect("exact post-fee reopen succeeds");
    let group = env.market_state().1;
    let accounts = [portfolios[0], portfolios[1], keeper].map(|key| env.portfolio_state(key));
    for party in 0..2 {
        let account = &accounts[party];
        let cert = health_cert(account);
        assert_eq!(account.capital.get(), DEPOSITS[party] - COLLECTED - PENDING);
        assert_eq!(account.last_fee_slot.get(), ADMISSION);
        assert_eq!(account.fee_credits.get(), 0);
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(account)),
            1
        );
        let leg = active_leg_for_asset(account, 0);
        assert_eq!(leg.basis_pos_q.unsigned_abs(), REOPEN_SIZE as u128);
        assert_eq!(
            leg.side,
            if party == 0 {
                SideV16::Long
            } else {
                SideV16::Short
            }
        );
        assert_eq!(cert.certified_equity, (DEPOSITS[party] - FEE) as i128);
        assert_eq!(cert.certified_initial_req, requirement(REOPEN_SIZE));
        assert!(assert_current_certificate_matches_independent(
            "flat reopen after fee history",
            &group,
            account
        )
        .unwrap());
    }
    assert_eq!(
        health_cert(&accounts[0]).certified_equity,
        health_cert(&accounts[0]).certified_initial_req as i128
    );
    assert_eq!(group.insurance, 2 * (COLLECTED + PENDING));
    assert_eq!(group.c_tot, DEPOSITS.iter().sum::<u128>() - group.insurance);
    assert_eq!(group.vault, DEPOSITS.iter().sum::<u128>());
    assert_eq!(
        (
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q
        ),
        (REOPEN_SIZE as u128, REOPEN_SIZE as u128)
    );
    assert_eq!(group.pnl_pos_tot, 0);
    assert_eq!(group.source_claim_bound_total_num, 0);
    assert_market_stock_census(
        "flat reopen after fee history",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &accounts,
        env.token_amount(env.vault).into(),
    )
    .unwrap();
    assert_reservation_encumbrance_census("flat reopen after fee history", &group, &accounts)
        .unwrap();
    for (index, key) in tracked.iter().enumerate() {
        if ![env.market, portfolios[0], portfolios[1]].contains(key) {
            assert_eq!(
                env.svm.get_account(key),
                before[index],
                "unrelated Account {key}"
            );
        }
    }
    let peak_cu = failure
        .meta
        .compute_units_consumed
        .max(admitted.compute_units_consumed);
    assert_cu_within("flat reopen fee/refresh bundle", peak_cu, 400_000);
    eprintln!("INV-027 flat fee history: public open/close, 1 prefix rollback, 1 exact reopen; peak CU={peak_cu}");
}

#[test]
fn v16_program_joint_accrued_liabilities_precede_risk_admission() {
    let lag = OLD_SIZE.unsigned_abs().div_ceil(POS_SCALE);
    let exact_req = requirement(OLD_SIZE) + lag + requirement(NEW_SIZE);
    let excess_req = requirement(OLD_SIZE) + lag + requirement(NEW_SIZE + 1);
    let thin_deposit = exact_req + FEE;
    assert_eq!((FEE, lag, exact_req, excess_req), (21, 11, 122, 123));
    assert!(
        thin_deposit >= excess_req,
        "omitting fees admits excess risk"
    );
    assert!(
        exact_req >= excess_req - lag,
        "omitting lag admits excess risk"
    );
    assert_eq!(
        excess_req - lag + OLD_SIZE.unsigned_abs() / POS_SCALE,
        exact_req,
        "flooring lag admits excess risk"
    );

    let mut worlds = 0;
    let mut max_cu = [0; 4]; // observation, explicit settlement, rejection, admission
    for thin_party in 0..2 {
        let mut settled_certificates = None;
        for route in [
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ] {
            for explicit_settlement in [true, false] {
                let label = format!("{route:?}/thin={thin_party}/settled={explicit_settlement}");
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        maintenance_margin_bps: MARGIN_BPS,
                        initial_margin_bps: MARGIN_BPS,
                        max_price_move_bps_per_slot: 500,
                        maintenance_fee_per_slot: FEE_RATE,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(START);
                for asset in 0..2 {
                    env.configure_auth_mark_for_asset_as_admin(asset, START, PRICE);
                }
                let owners = [Keypair::new(), Keypair::new()];
                let portfolios = owners
                    .each_ref()
                    .map(|owner| public_portfolio(&mut env, owner));
                let keeper_owner = Keypair::new();
                let keeper = public_portfolio(&mut env, &keeper_owner);
                let deposits = std::array::from_fn::<_, 2, _>(|party| {
                    if party == thin_party {
                        thin_deposit
                    } else {
                        10_000
                    }
                });
                let tokens = std::array::from_fn::<_, 2, _>(|party| {
                    public_deposit(&mut env, &owners[party], portfolios[party], deposits[party])
                });
                env.trade_asset_with_cu(
                    1,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    OLD_SIZE,
                    PRICE,
                    0,
                );
                let matcher = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi).then(|| {
                    auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1])
                });
                let original_portfolios = portfolios.map(|key| env.svm.get_account(&key));
                let original_group = env.market_state().1;

                // Advance only the empty keeper at unchanged prices. Then change the nontraded
                // target in the same slot, isolating lag from marked PnL and funding accrual.
                for slot in START + 1..=ADMISSION {
                    env.svm.warp_to_slot(slot);
                    let cu = env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations_for_assets(&[0, 1]),
                        },
                    );
                    max_cu[0] = max_cu[0].max(cu);
                }
                let target = if thin_party == 0 {
                    PRICE - 1
                } else {
                    PRICE + 1
                };
                env.push_auth_mark_for_asset_as_admin(1, ADMISSION, target);
                let cu = env.crank(
                    keeper,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: ADMISSION,
                        observations: crank_observations(1),
                    },
                );
                max_cu[0] = max_cu[0].max(cu);
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    original_portfolios,
                    "{label}"
                );
                let pending = env.market_state().1;
                assert_eq!(pending.current_slot, ADMISSION);
                assert_eq!(pending.insurance, 0);
                assert_eq!(pending.c_tot, deposits.iter().sum());
                for asset in 0..2 {
                    let old = original_group.assets[asset];
                    let new = pending.assets[asset];
                    assert_eq!(new.slot_last, ADMISSION);
                    assert_eq!(new.effective_price, PRICE);
                    assert_eq!(
                        new.raw_oracle_target_price,
                        if asset == 1 { target } else { PRICE }
                    );
                    assert_eq!(
                        (new.k_long, new.k_short, new.f_long_num, new.f_short_num),
                        (old.k_long, old.k_short, old.f_long_num, old.f_short_num)
                    );
                }
                for party in 0..2 {
                    let account = env.portfolio_state(portfolios[party]);
                    assert_eq!(account.capital.get(), deposits[party]);
                    assert_eq!(account.last_fee_slot.get(), START);
                    assert_eq!(account.fee_credits.get(), 0);
                    assert_eq!(account.pnl.get(), 0);
                    assert!(!has_active_leg_for_asset(&account, 0));
                    assert_eq!(
                        health_cert(&account).certified_initial_req,
                        requirement(OLD_SIZE)
                    );
                    assert!(health_cert(&account).cert_oracle_epoch < pending.oracle_epoch);
                }

                let trade = |env: &mut V16CuEnv, size_q| {
                    let ix = match route {
                        TradeRoute::NoCpi => {
                            env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, size_q, PRICE, 0)
                        }
                        TradeRoute::Cpi => {
                            env.trade_cpi_ix(portfolios[0], portfolios[1], 0, size_q, 0, PRICE)
                        }
                        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q,
                                exec_price: PRICE,
                                fee_bps: 0,
                            }],
                        ),
                        TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeCpiLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q,
                                fee_bps: 0,
                                limit_price: PRICE,
                            }],
                            0,
                            0,
                        ),
                    };
                    let mut accounts = vec![AccountMeta::new(owners[0].pubkey(), true)];
                    let mut signers = vec![&env.payer, &owners[0]];
                    if matcher.is_none() {
                        accounts.push(AccountMeta::new(owners[1].pubkey(), true));
                        signers.push(&owners[1]);
                    }
                    accounts.extend([
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ]);
                    if let Some((program, context, delegate)) = matcher {
                        accounts.extend([
                            AccountMeta::new_readonly(program, false),
                            AccountMeta::new(context, false),
                            AccountMeta::new_readonly(delegate, false),
                        ]);
                    }
                    env.svm.expire_blockhash();
                    let tx = Transaction::new_signed_with_payer(
                        &[
                            heap_ix(),
                            cu_ix(),
                            Instruction {
                                program_id: env.program_id,
                                accounts,
                                data: ix.encode(),
                            },
                        ],
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    env.svm.send_transaction(tx)
                };
                let mut tracked = vec![
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    keeper,
                    env.mint,
                    env.vault,
                    tokens[0],
                    tokens[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    env.program_id,
                    spl_token::ID,
                ];
                if let Some((program, context, delegate)) = matcher {
                    tracked.extend([program, context, delegate]);
                }
                let snapshot = |env: &V16CuEnv| {
                    tracked
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>()
                };
                let original_frame = snapshot(&env);

                let check = |env: &V16CuEnv, admitted: bool| {
                    let group = env.market_state().1;
                    let total: u128 = deposits.iter().sum();
                    assert_eq!(group.vault, total, "{label}");
                    assert_eq!(group.c_tot, total - 2 * FEE, "{label}");
                    assert_eq!(group.insurance, 2 * FEE, "{label}");
                    assert_eq!(group.vault, group.c_tot + group.insurance, "{label}");
                    assert_eq!(
                        group.vault,
                        u128::from(env.token_amount(env.vault)),
                        "{label}"
                    );
                    assert_eq!(
                        u128::from(
                            Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                                .unwrap()
                                .supply
                        ),
                        total
                    );
                    assert_eq!(tokens.map(|key| env.token_amount(key)), [0, 0]);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    // Account-level maintenance belongs to the canonical base insurance,
                    // independently of the old/new leg or the party bearing adverse lag.
                    for (domain, expected) in [2 * (FEE / 2), 2 * (FEE - FEE / 2), 0, 0]
                        .into_iter()
                        .enumerate()
                    {
                        assert_eq!(
                            group.insurance_domain_budget[domain], expected,
                            "{label}: fee destination"
                        );
                    }
                    std::array::from_fn::<_, 2, _>(|party| {
                        let account = env.portfolio_state(portfolios[party]);
                        let cert = health_cert(&account);
                        let penalty = if party == thin_party { lag } else { 0 };
                        let req = requirement(OLD_SIZE)
                            + penalty
                            + if admitted { requirement(NEW_SIZE) } else { 0 };
                        assert_eq!(account.capital.get(), deposits[party] - FEE, "{label}");
                        assert_eq!(account.last_fee_slot.get(), ADMISSION, "{label}");
                        assert_eq!(account.fee_credits.get(), 0, "{label}");
                        assert_eq!(account.pnl.get(), 0, "{label}");
                        assert_eq!(
                            cert.certified_equity,
                            (deposits[party] - FEE) as i128,
                            "{label}"
                        );
                        assert_eq!(cert.certified_initial_req, req, "{label}");
                        assert_eq!(cert.certified_maintenance_req, req, "{label}");
                        assert_eq!(
                            cert.certified_worst_case_loss,
                            1_010 + penalty + if admitted { 100 } else { 0 },
                            "{label}"
                        );
                        assert_eq!(cert.certified_liq_deficit, 0, "{label}");
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(&account)),
                            if admitted { 2 } else { 1 }
                        );
                        assert!(assert_current_certificate_matches_independent(
                            &label, &group, &account
                        )
                        .expect(
                            "all current certificate lanes and keys match independent accounting"
                        ));
                        cert
                    })
                };
                if explicit_settlement {
                    for portfolio in portfolios {
                        let cu = env.crank(
                            portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: ADMISSION,
                                observations: crank_observations_for_assets(&[0, 1]),
                            },
                        );
                        max_cu[1] = max_cu[1].max(cu);
                    }
                    check(&env, false);
                }

                let before = snapshot(&env);
                let failure = trade(&mut env, NEW_SIZE + 1)
                    .expect_err("both obligations precede risk admission");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)
                    ),
                    "{label}: {failure:?}"
                );
                max_cu[2] = max_cu[2].max(failure.meta.compute_units_consumed);
                assert_eq!(
                    snapshot(&env),
                    before,
                    "{label}: exact rejection rollback, including matcher"
                );

                let meta =
                    trade(&mut env, NEW_SIZE).expect("joint fee/lag exact boundary must admit");
                max_cu[3] = max_cu[3].max(meta.compute_units_consumed);
                let certs = check(&env, true);
                assert_eq!(
                    certs[thin_party].certified_equity,
                    certs[thin_party].certified_initial_req as i128
                );
                if let Some(expected) = settled_certificates {
                    assert_eq!(
                        certs, expected,
                        "{label}: direct admission equals explicit public settlement"
                    );
                } else {
                    assert!(explicit_settlement);
                    settled_certificates = Some(certs);
                }
                let group = env.market_state().1;
                for (asset, size) in [(0, NEW_SIZE), (1, OLD_SIZE)] {
                    assert_eq!(group.assets[asset as usize].oi_eff_long_q, size as u128);
                    assert_eq!(group.assets[asset as usize].oi_eff_short_q, size as u128);
                    for party in 0..2 {
                        assert_eq!(
                            active_leg_for_asset(&env.portfolio_state(portfolios[party]), asset)
                                .basis_pos_q,
                            if party == 0 { size } else { -size }
                        );
                    }
                }
                assert_eq!(
                    group.assets[1], pending.assets[1],
                    "{label}: nontraded lag remains in force"
                );
                for (key, original) in tracked.iter().zip(original_frame) {
                    if *key != env.market
                        && !portfolios.contains(key)
                        && !matcher.is_some_and(|(_, context, _)| *key == context)
                    {
                        assert_eq!(
                            env.svm.get_account(key),
                            original,
                            "{label}: custody/unrelated frame {key}"
                        );
                    }
                }
                for portfolio in portfolios {
                    let before = snapshot(&env);
                    assert_eq!(
                        env.crank_if_actionable(
                            portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: ADMISSION,
                                observations: crank_observations_for_assets(&[0, 1]),
                            }
                        ),
                        None,
                        "{label}: settled fees and current health are a fixed point"
                    );
                    assert_eq!(
                        snapshot(&env),
                        before,
                        "{label}: fixed-point crank cannot charge or refresh twice"
                    );
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    for (label, cu, limit) in [
        ("joint liability observation", max_cu[0], CRANK_CU_LIMIT),
        ("joint liability settlement", max_cu[1], CRANK_CU_LIMIT),
        (
            "joint liability rejection",
            max_cu[2],
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        ),
        (
            "joint liability admission",
            max_cu[3],
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        ),
    ] {
        assert_cu_within(label, cu, limit);
    }
    eprintln!("INV-027 joint liabilities: {worlds} worlds, 16 exact rejections, 16 exact admissions, 32 fixed-point rollbacks; max CU [observation, settlement, rejection, admission]={max_cu:?}");
}
