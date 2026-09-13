//! INV-027/024/053/080: standalone first admission followed by exact deferred-fee
//! collection and owner exit. Both owners remain solvent after all elapsed fees;
//! this does not certify admission at a margin boundary with uncollected fees.
//! System/SPL/ATA/wrapper instructions create every economic account.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_standalone_first_admission_preserves_deferred_fee_owner_entitlement() {
    const BIRTH: [u64; 2] = [1, 2];
    const OPEN: u64 = 6;
    const FUNDS: [u128; 2] = [2_000, 1_000];
    const QUANTITY: i128 = (POS_SCALE + POS_SCALE / 10) as i128;
    const TRADE_BPS: u64 = 100;

    let maintenance = BIRTH.map(|slot| FEE_RATE * u128::from(OPEN - slot));
    let notional = (QUANTITY.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    let trade_fee = (notional * u128::from(TRADE_BPS)).div_ceil(10_000);
    let entitlement = std::array::from_fn::<_, 2, _>(|i| FUNDS[i] - maintenance[i] - trade_fee);
    let insurance = maintenance.iter().sum::<u128>() + 2 * trade_fee;
    assert_eq!((maintenance, notional, trade_fee), ([35, 28], 110, 2));
    assert_eq!((entitlement, insurance), ([1_963, 970], 67));
    assert!(entitlement.iter().all(|&capital| capital > notional));

    let mut worlds = 0;
    let mut peak_cu = 0;
    for batch in [false, true] {
        for direction in [-1, 1] {
            let label = format!("standalone first admission/batch={batch}/direction={direction}");
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
            let keeper_owner = Keypair::new();
            let keeper = public_portfolio(&mut env, &keeper_owner);
            let owners = [Keypair::new(), Keypair::new()];
            let mut portfolios = [Pubkey::default(); 2];
            let mut tokens = [Pubkey::default(); 2];
            for i in 0..2 {
                env.svm.warp_to_slot(BIRTH[i]);
                portfolios[i] = public_portfolio(&mut env, &owners[i]);
                tokens[i] = public_deposit(&mut env, &owners[i], portfolios[i], FUNDS[i]);
            }
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
            assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), funded);
            let group = env.market_state().1;
            assert_eq!(
                (group.current_slot, group.assets[0].slot_last),
                (OPEN, OPEN)
            );
            for (i, portfolio) in portfolios.iter().enumerate() {
                let account = env.portfolio_state(*portfolio);
                assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                assert_eq!(account.last_fee_slot.get(), BIRTH[i]);
                assert_eq!(account.capital.get(), FUNDS[i]);
            }

            let tracked = [
                env.market,
                portfolios[0],
                portfolios[1],
                keeper,
                env.vault,
                env.mint,
                tokens[0],
                tokens[1],
                owners[0].pubkey(),
                owners[1].pubkey(),
                keeper_owner.pubkey(),
                env.admin.pubkey(),
                env.vault_authority,
            ];
            let unchanged = [
                keeper,
                env.mint,
                owners[0].pubkey(),
                owners[1].pubkey(),
                keeper_owner.pubkey(),
                env.admin.pubkey(),
                env.vault_authority,
            ];
            let unchanged_accounts = unchanged.map(|key| env.svm.get_account(&key));
            let check =
                |env: &V16CuEnv, admitted: bool, open: bool, collected: bool, paid: [u128; 2]| {
                    let group = env.market_state().1;
                    let accounts =
                        [portfolios[0], portfolios[1], keeper].map(|key| env.portfolio_state(key));
                    let fee = if admitted { trade_fee } else { 0 };
                    let elapsed = if collected { maintenance } else { [0; 2] };
                    let insurance = 2 * fee + elapsed.iter().sum::<u128>();
                    assert_eq!(group.insurance, insurance, "{label}");
                    assert_eq!(
                        group.c_tot,
                        FUNDS.iter().sum::<u128>() - insurance - paid.iter().sum::<u128>()
                    );
                    assert_eq!(
                        group.vault,
                        FUNDS.iter().sum::<u128>() - paid.iter().sum::<u128>()
                    );
                    assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                    assert_eq!(group.vault, group.c_tot + group.insurance);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(
                        group.insurance_domain_budget[0],
                        fee + elapsed.iter().map(|amount| amount / 2).sum::<u128>()
                    );
                    assert_eq!(
                        group.insurance_domain_budget[1],
                        fee + elapsed
                            .iter()
                            .map(|amount| amount - amount / 2)
                            .sum::<u128>()
                    );
                    assert!(group.insurance_domain_budget[2..]
                        .iter()
                        .all(|&amount| amount == 0));
                    assert_eq!(
                        (
                            group.assets[0].effective_price,
                            group.assets[0].raw_oracle_target_price
                        ),
                        (PRICE, PRICE)
                    );
                    for i in 0..2 {
                        let account = &accounts[i];
                        assert_eq!(
                            account.capital.get(),
                            FUNDS[i] - fee - elapsed[i] - paid[i],
                            "{label}: owner {i}"
                        );
                        assert_eq!(
                            account.last_fee_slot.get(),
                            if collected { OPEN } else { BIRTH[i] }
                        );
                        assert_eq!(account.fee_credits.get(), 0);
                        assert_eq!(account.pnl.get(), 0);
                        assert_eq!(u128::from(env.token_amount(tokens[i])), paid[i]);
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(account)),
                            u32::from(open)
                        );
                        let current =
                            assert_current_certificate_matches_independent(&label, &group, account)
                                .unwrap();
                        if open {
                            assert!(current, "{label}: admission certifies both owners");
                            let cert = health_cert(account);
                            assert_eq!(cert.certified_initial_req, notional);
                            assert_eq!(cert.certified_maintenance_req, notional / 2);
                            assert_eq!(cert.certified_liq_deficit, 0);
                            let leg = active_leg_for_asset(account, 0);
                            assert_eq!(leg.basis_pos_q.unsigned_abs(), QUANTITY.unsigned_abs());
                            assert_eq!(
                                leg.side,
                                if (i == 0) == (direction > 0) {
                                    SideV16::Long
                                } else {
                                    SideV16::Short
                                }
                            );
                        }
                    }
                    let oi = if open { QUANTITY.unsigned_abs() } else { 0 };
                    assert_eq!(
                        (
                            group.assets[0].oi_eff_long_q,
                            group.assets[0].oi_eff_short_q
                        ),
                        (oi, oi)
                    );
                    assert_eq!(accounts[2].capital.get(), 0);
                    assert_eq!(accounts[2].pnl.get(), 0);
                    assert_eq!(
                        unchanged.map(|key| env.svm.get_account(&key)),
                        unchanged_accounts
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
                    assert_source_credit_rates(&label, &group).unwrap();
                };
            check(&env, false, false, false, [0; 2]);

            let trade = |env: &V16CuEnv, quantity, fee_bps| Instruction {
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
                            size_q: quantity,
                            exec_price: PRICE,
                            fee_bps,
                        }],
                    )
                } else {
                    env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, quantity, PRICE, fee_bps)
                }
                .encode(),
            };
            let withdrawal = |env: &V16CuEnv, i: usize, owner: usize, amount| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(owners[owner].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[owner], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(portfolios[i], amount).encode(),
            };
            let mut submit =
                |env: &mut V16CuEnv, instructions: Vec<Instruction>, rejects: bool, cu_limit| {
                    env.svm.expire_blockhash();
                    let mut all = vec![heap_ix(), cu_ix()];
                    all.extend(instructions);
                    let mut signers = vec![&env.payer];
                    signers.extend(owners.iter().filter(|owner| {
                        all.iter().any(|ix| {
                            ix.accounts
                                .iter()
                                .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
                        })
                    }));
                    let tx = Transaction::new_signed_with_payer(
                        &all,
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    let mut keys = tracked.to_vec();
                    keys.extend(tx.message.account_keys.iter().copied());
                    keys.sort_unstable();
                    keys.dedup();
                    let mut before: Vec<_> =
                        keys.iter().map(|key| env.svm.get_account(key)).collect();
                    let network_fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    let cu = if rejects {
                        let error = env
                            .svm
                            .send_transaction(tx)
                            .expect_err("wrong portfolio owner rejects after the valid trade");
                        assert_eq!(
                            error.err,
                            TransactionError::InstructionError(
                                3,
                                InstructionError::Custom(PercolatorError::Unauthorized as u32)
                            ),
                            "{label}"
                        );
                        let payer = keys
                            .iter()
                            .position(|key| *key == env.payer.pubkey())
                            .unwrap();
                        before[payer].as_mut().unwrap().lamports -= network_fee;
                        assert_eq!(
                            keys.iter()
                                .map(|key| env.svm.get_account(key))
                                .collect::<Vec<_>>(),
                            before,
                            "{label}: complete rollback, including the successful prefix"
                        );
                        error.meta.compute_units_consumed
                    } else {
                        env.svm
                            .send_transaction(tx)
                            .expect("public owner route succeeds")
                            .compute_units_consumed
                    };
                    assert_cu_within(&label, cu, cu_limit);
                    peak_cu = peak_cu.max(cu);
                };

            let open = trade(&env, direction * QUANTITY, TRADE_BPS);
            let bad_owner = withdrawal(&env, 0, 1, 1);
            submit(
                &mut env,
                vec![open.clone(), bad_owner],
                true,
                TRADE_CU_LIMIT + CUSTODY_CU_LIMIT,
            );
            check(&env, false, false, false, [0; 2]);
            // The successful admission contains exactly one wrapper instruction.
            submit(&mut env, vec![open], false, TRADE_CU_LIMIT);
            check(&env, true, true, false, [0; 2]);

            let close = trade(&env, -direction * QUANTITY, 0);
            let bad_owner = withdrawal(&env, 0, 1, 1);
            submit(
                &mut env,
                vec![close.clone(), bad_owner],
                true,
                TRADE_CU_LIMIT + CUSTODY_CU_LIMIT,
            );
            check(&env, true, true, false, [0; 2]);
            submit(&mut env, vec![close], false, TRADE_CU_LIMIT);
            check(&env, true, false, true, [0; 2]);

            let mut paid = [0; 2];
            let order = if direction > 0 { [0, 1] } else { [1, 0] };
            for i in order {
                let payout = withdrawal(&env, i, i, entitlement[i]);
                submit(&mut env, vec![payout], false, CUSTODY_CU_LIMIT);
                paid[i] = entitlement[i];
                check(&env, true, false, true, paid);
            }
            assert_eq!(env.market_state().1.c_tot, 0);
            assert_eq!(env.token_amount(env.vault) as u128, insurance);
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    println!("standalone first admission: worlds={worlds}, exact_rollbacks={}, owner_payouts={}, insurance_per_world={insurance}, peak_cu={peak_cu}", 2 * worlds, 2 * worlds);
}
