//! INV-039/024/067: pending cohorts retain their original debt while each owner's
//! maintenance stops at resolution. Delayed settlement, retry and insurance exit
//! reconcile fees separately from loss weight, receipts and user SPL payouts.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const RATE: u128 = 7;
const RESOLVED: u64 = 23;
const DEBTS: [u128; 2] = [3 * 7, 2 * 13_999];
const SUPPLY: u128 = 930_777;
const TOTAL_FEES: u128 = 5 * RATE * RESOLVED as u128;

struct FeeModel {
    basis: [i128; 4],
    pending: [bool; 4],
    fee_slots: [u64; 5],
    budgets: [u128; 2],
}

impl FeeModel {
    fn settle(&mut self, actor: usize) {
        let fee = RATE * u128::from(RESOLVED - self.fee_slots[actor]);
        self.budgets[0] += fee / 2;
        self.budgets[1] += fee - fee / 2;
        self.fee_slots[actor] = RESOLVED;
        if actor < 4 {
            self.basis[actor] = 0;
            self.pending[actor] = false;
        }
    }

    fn check(&self, world: &AttributionWorld) {
        world.check(self.basis, self.pending);
        let env = &world.env;
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.resolved_slot, RESOLVED);
        assert_eq!(group.insurance, self.budgets.iter().sum::<u128>());
        assert_eq!(group.insurance_domain_budget[..2], self.budgets);
        assert!(group.insurance_domain_budget[2..].iter().all(|x| *x == 0));
        assert!(group.insurance_domain_spent.iter().all(|x| *x == 0));
        for (actor, a) in world.actors.iter().enumerate() {
            let account = env.portfolio_state(a.portfolio);
            assert_eq!(account.last_fee_slot.get(), self.fee_slots[actor]);
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert_eq!(receipt.terminal_positive_claim_face, DEBTS[actor / 2]);
                receipt.terminal_positive_claim_face - receipt.paid_effective
            } else {
                0
            };
            let pnl = match actor {
                0 | 2 => DEBTS[actor / 2] as i128,
                1 | 3 if self.basis[actor] == 0 => -(DEBTS[actor / 2] as i128),
                _ => 0,
            };
            let expected = ATTRIBUTION_DEPOSITS[actor] as i128 + pnl
                - (RATE * u128::from(self.fee_slots[actor])) as i128;
            assert_eq!(
                account.capital.get() as i128
                    + account.pnl.get()
                    + due as i128
                    + i128::from(env.token_amount(a.token)),
                expected,
                "actor {actor}: own fee and original debt remain separately attributed"
            );
            let token_account = env.svm.get_account(&a.token).unwrap();
            let token = TokenAccount::unpack(&token_account.data).unwrap();
            assert_eq!(token_account.owner, spl_token::ID);
            assert_eq!((token.owner, token.mint), (a.owner.pubkey(), env.mint));
            assert_eq!(
                (token.delegate, token.close_authority, token.is_native),
                (COption::None, COption::None, COption::None)
            );
            if matches!(actor, 0 | 2) && self.basis[actor + 1] != 0 {
                assert_eq!(account.pnl.get(), DEBTS[actor / 2] as i128);
                assert_eq!(env.token_amount(a.token), 0);
                assert!(!receipt.present);
                assert!(!group.payout_snapshot_captured);
            }
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply as u128, SUPPLY);
    }
}

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn payout(world: &AttributionWorld, actor: usize) -> Instruction {
    let env = &world.env;
    let a = &world.actors[actor];
    wrap(
        env,
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(a.owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a.portfolio, false),
            AccountMeta::new(a.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    )
}

fn land(
    world: &mut AttributionWorld,
    ixs: &[Instruction],
    signers: &[&Keypair],
    allowed: &[Pubkey],
    waiting_index: Option<u8>,
) -> u64 {
    world.env.svm.expire_blockhash();
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(ixs);
    let env = &mut world.env;
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        1 + signers.len()
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let env = &mut world.env;
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = waiting_index {
        let failure = result.expect_err("other cohort still has unbooked debt");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
            )
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|log| **log == format!("Program {} success", env.program_id))
                .count(),
            usize::from(index - 2)
        );
        if index > 2 {
            assert!(
                failure
                    .meta
                    .logs
                    .contains(&format!("Program {} success", spl_token::ID)),
                "the debtor prefix must actually transfer SPL value before rollback"
            );
        }
        failure.meta
    } else {
        result.expect("public terminal continuation")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if waiting_index.is_some() || !allowed.contains(&key) {
            assert_eq!(
                env.svm.get_account(&key),
                expected,
                "exact Account frame {key}"
            );
        }
    }
    assert_cu_within(
        "INV-039 pending terminal fees",
        meta.compute_units_consumed,
        400_000,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_pending_cohort_terminal_fees_stop_at_resolution_and_reach_insurance_exit() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    for reverse in [false, true] {
        for debtor_order in [[1, 3], [3, 1]] {
            for delay in [0, 30] {
                let mut world = AttributionWorld::new_with_params(
                    reverse,
                    V16CuMarketParams {
                        max_portfolio_assets: 3,
                        max_abs_funding_e9_per_slot: 0,
                        liquidation_fee_bps: 0,
                        maintenance_fee_per_slot: RATE,
                        ..production_risk_params()
                    },
                );
                let sign = if reverse { -1 } else { 1 };
                for (pair, lots) in [3, 2].into_iter().enumerate() {
                    let q = sign * lots * POS_SCALE as i128;
                    world.quantities[2 * pair] = q;
                    world.quantities[2 * pair + 1] = -q;
                    world.env.trade_asset_with_cu(
                        (pair + 1) as u16,
                        &world.actors[2 * pair].owner,
                        world.actors[2 * pair].portfolio,
                        &world.actors[2 * pair + 1].owner,
                        world.actors[2 * pair + 1].portfolio,
                        q,
                        1_000_000,
                        0,
                    );
                }
                world.env.svm.warp_to_slot(20);
                for (pair, movement) in [7, 13_999].into_iter().enumerate() {
                    world.env.push_auth_mark_for_asset_as_admin(
                        (pair + 1) as u16,
                        20,
                        (1_000_000 + sign * movement) as u64,
                    );
                }
                world.env.crank(
                    world.actors[4].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 20,
                        observations: crank_observations_for_assets(&[1, 2]),
                    },
                );
                for pair in 0..2 {
                    world.env.crank(
                        world.actors[2 * pair].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 20,
                            observations: crank_observations((pair + 1) as u16),
                        },
                    );
                    world.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        (pair + 1) as u16,
                        20,
                        0,
                    );
                    world.forfeit(2 * pair);
                }
                let mut model = FeeModel {
                    basis: [0, world.quantities[1], 0, world.quantities[3]],
                    pending: [true, false, true, false],
                    fee_slots: [20, 0, 20, 0, 0],
                    budgets: [140, 140],
                };
                let admin = world.env.admin.insecure_clone();
                send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &world.env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                world.env.svm.warp_to_slot(RESOLVED);
                world.env.resolve();
                model.check(&world);
                let payouts: [Instruction; 5] = std::array::from_fn(|actor| payout(&world, actor));
                world.env.svm.warp_to_slot(RESOLVED + 5 + delay);
                for holder in [0, 2] {
                    let allowed = [world.env.market, world.actors[holder].portfolio];
                    peak_cu = peak_cu.max(land(
                        &mut world,
                        &[payouts[holder].clone()],
                        &[],
                        &allowed,
                        None,
                    ));
                    model.settle(holder);
                    model.check(&world);
                }
                assert_eq!(model.pending, [false; 4]);
                assert_eq!(model.fee_slots, [RESOLVED, 0, RESOLVED, 0, 0]);

                world.env.svm.warp_to_slot(RESOLVED + 9 + delay);
                let first = debtor_order[0];
                let waiting_holder = debtor_order[1] - 1;
                peak_cu = peak_cu.max(land(
                    &mut world,
                    &[payouts[first].clone(), payouts[waiting_holder].clone()],
                    &[],
                    &[],
                    Some(3),
                ));
                model.check(&world);
                for debtor in debtor_order {
                    let allowed = [
                        world.env.market,
                        world.env.vault,
                        world.actors[debtor].portfolio,
                        world.actors[debtor].token,
                    ];
                    peak_cu = peak_cu.max(land(
                        &mut world,
                        &[payouts[debtor].clone()],
                        &[],
                        &allowed,
                        None,
                    ));
                    model.settle(debtor);
                    model.check(&world);
                    world
                        .env
                        .svm
                        .warp_to_slot(world.env.svm.get_sysvar::<Clock>().slot + 11);
                }
                for actor in [waiting_holder, first - 1, 4] {
                    let allowed = [
                        world.env.market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        world.actors[actor].token,
                    ];
                    peak_cu = peak_cu.max(land(
                        &mut world,
                        &[payouts[actor].clone()],
                        &[],
                        &allowed,
                        None,
                    ));
                    model.settle(actor);
                    model.check(&world);
                }
                assert_eq!(model.fee_slots, [RESOLVED; 5]);
                assert_eq!(model.budgets, [400, 405]);
                assert_eq!(world.env.market_state().1.vault, TOTAL_FEES);
                let expected: [u64; 5] = std::array::from_fn(|actor| {
                    let pnl = match actor {
                        0 | 2 => DEBTS[actor / 2] as i128,
                        1 | 3 => -(DEBTS[actor / 2] as i128),
                        _ => 0,
                    };
                    (ATTRIBUTION_DEPOSITS[actor] as i128 + pnl - (RATE * RESOLVED as u128) as i128)
                        as u64
                });
                assert_eq!(expected, [199_860, 179_818, 327_837, 221_841, 616]);
                world.env.svm.warp_to_slot(100 + delay);
                for actor in 0..5 {
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                    peak_cu = peak_cu.max(land(
                        &mut world,
                        &[payouts[actor].clone()],
                        &[],
                        &[],
                        Some(2),
                    ));
                    model.check(&world);
                }

                // Portfolio rent enters the slab; fees stay in SPL custody for the beneficiary.
                for actor in 0..5 {
                    let a = &world.actors[actor];
                    let owner = a.owner.insecure_clone();
                    let portfolio = a.portfolio;
                    let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                    let slab_lamports = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let ix = wrap(
                        &world.env,
                        world.env.close_portfolio_ix(portfolio),
                        vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(world.env.market, false),
                            AccountMeta::new(portfolio, false),
                        ],
                    );
                    let allowed = [world.env.market, portfolio];
                    peak_cu = peak_cu.max(land(&mut world, &[ix], &[&owner], &allowed, None));
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        slab_lamports + rent
                    );
                    assert!(world
                        .env
                        .svm
                        .get_account(&portfolio)
                        .map_or(true, |a| a.lamports == 0 && a.data.is_empty()));
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                let destination = create_ata_for_test(
                    &mut world.env.svm,
                    &world.env.payer,
                    admin.pubkey(),
                    world.env.mint,
                );
                let withdraw = wrap(
                    &world.env,
                    ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id: world.env.asset_market_id(0),
                        authority_epoch: world.env.control_sequences(0).authority_epoch,
                        amount: TOTAL_FEES,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(world.env.vault, false),
                        AccountMeta::new_readonly(world.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                let allowed = [world.env.market, world.env.vault, destination];
                peak_cu = peak_cu.max(land(&mut world, &[withdraw], &[&admin], &allowed, None));
                let group = world.env.market_state().1;
                assert_eq!(
                    (group.vault, group.insurance, group.c_tot, group.pnl_pos_tot),
                    (0, 0, 0, 0)
                );
                assert_eq!(world.env.token_amount(destination) as u128, TOTAL_FEES);
                assert_eq!(
                    expected.iter().map(|x| u128::from(*x)).sum::<u128>() + TOTAL_FEES,
                    SUPPLY
                );

                let close = wrap(
                    &world.env,
                    ProgInstruction::CloseSlab {
                        authority_epoch: world.env.control_sequences(0).authority_epoch,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.env.vault, false),
                        AccountMeta::new_readonly(world.env.vault_authority, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(world.env.mint, false),
                    ],
                );
                let slab_rent = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let vault_rent = world
                    .env
                    .svm
                    .get_account(&world.env.vault)
                    .unwrap()
                    .lamports;
                let mut expected_admin = world.env.svm.get_account(&admin.pubkey()).unwrap();
                let allowed = [world.env.market, world.env.vault, admin.pubkey()];
                for _ in 0..4 {
                    peak_cu = peak_cu.max(land(
                        &mut world,
                        &[close.clone()],
                        &[&admin],
                        &allowed,
                        None,
                    ));
                    if world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .data
                        .len()
                        == percolator_prog::constants::HEADER_LEN
                    {
                        break;
                    }
                }
                let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                assert_eq!(
                    tombstone.lamports,
                    world
                        .env
                        .svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
                );
                expected_admin.lamports += slab_rent + vault_rent - tombstone.lamports;
                assert_eq!(
                    world.env.svm.get_account(&admin.pubkey()),
                    Some(expected_admin)
                );
                assert!(world
                    .env
                    .svm
                    .get_account(&world.env.vault)
                    .map_or(true, |a| a.lamports == 0 && a.data.is_empty()));
                assert_eq!(world.env.token_amount(destination) as u128, TOTAL_FEES);
                for (a, paid) in world.actors.iter().zip(expected) {
                    assert_eq!(world.env.token_amount(a.token), paid);
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    println!("INV-039 terminal fees: {worlds} worlds, 48 exact rollbacks, 40 user payouts, 8 insurance exits and slab closes; peak CU={peak_cu}");
}
