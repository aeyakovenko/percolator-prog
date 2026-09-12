//! INV-039: two unequal pending domains share one portfolio and terminal entitlement.
//! Public construction crosses resolution placement, debtor deletion order and
//! partial holder detachment. Removing one weight must not discharge the other
//! domain or turn the combined claim into an early payout. Early debt becomes
//! junior receipt face; late debt is source-realized. Their sum is unchanged.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const LOTS: [u128; 2] = [1, 2];
const MOVES: [u128; 2] = [7, 13_999];
const DEBTORS: [usize; 2] = [1, 3];

fn debts() -> [u128; 2] {
    std::array::from_fn(|pair| LOTS[pair] * MOVES[pair])
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Close(usize),
    Claim(usize),
    Delete(usize),
}

impl Action {
    fn actor(self) -> usize {
        match self {
            Self::Close(actor) | Self::Claim(actor) | Self::Delete(actor) => actor,
        }
    }

    fn instruction(self, world: &AttributionWorld) -> Instruction {
        let env = &world.env;
        let actor = &world.actors[self.actor()];
        let (ix, accounts) = match self {
            Self::Delete(_) => (
                env.close_portfolio_ix(actor.portfolio),
                vec![
                    AccountMeta::new(actor.owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(actor.portfolio, false),
                ],
            ),
            _ => (
                if matches!(self, Self::Claim(_)) {
                    ProgInstruction::ClaimResolvedPayoutTopup
                } else {
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                },
                vec![
                    AccountMeta::new_readonly(actor.owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(actor.portfolio, false),
                    AccountMeta::new(actor.token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            ),
        };
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ix.encode(),
        }
    }
}

fn land(
    world: &mut AttributionWorld,
    actions: &[Action],
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    world.env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend(actions.iter().map(|action| action.instruction(world)));
    let mut owners: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            Action::Delete(actor) => Some(*actor),
            _ => None,
        })
        .collect();
    owners.sort_unstable();
    owners.dedup();
    let mut signers = vec![&world.env.payer];
    signers.extend(owners.iter().map(|&actor| &world.actors[actor].owner));
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        owners.len() + 1
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| (*key, world.env.svm.get_account(key)))
        .collect();
    let result = world.env.svm.send_transaction(tx);
    let meta = match rejection {
        Some((index, error)) => {
            let failure = result.expect_err("the unpaid common claim must block this suffix");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
            );
            for (key, mut account) in before {
                if key == world.env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "exact rollback: {key}"
                );
            }
            failure.meta
        }
        None => {
            let meta =
                result.unwrap_or_else(|error| panic!("public continuation {actions:?}: {error:?}"));
            for (key, account) in before {
                let may_change = key == world.env.payer.pubkey()
                    || key == world.env.market
                    || key == world.env.vault
                    || actions.iter().any(|action| {
                        let actor = &world.actors[action.actor()];
                        key == actor.portfolio || key == actor.token
                    });
                if !may_change {
                    assert_eq!(
                        world.env.svm.get_account(&key),
                        account,
                        "foreign account: {key}"
                    );
                }
            }
            meta
        }
    };
    assert_cu_within(
        "INV-039 common-holder transaction",
        meta.compute_units_consumed,
        if actions.len() == 1 {
            CUSTODY_CU_LIMIT
        } else {
            500_000
        },
    );
    meta.compute_units_consumed
}

struct Model {
    sign: i128,
    junior_face: u128,
    pending: [bool; 2],
    settled: [bool; 2],
    deleted: [bool; 5],
}

impl Model {
    fn check(&self, world: &AttributionWorld) {
        let env = &world.env;
        let group = env.market_state().1;
        let debt = debts();
        let mut capital = 0;
        let mut positive_pnl = 0;
        let mut expected_payouts = ATTRIBUTION_DEPOSITS;
        expected_payouts[0] += debt.iter().sum::<u128>();
        for pair in 0..2 {
            expected_payouts[DEBTORS[pair]] -= debt[pair];
        }
        for (actor, a) in world.actors.iter().enumerate() {
            let paid = u128::from(env.token_amount(a.token));
            assert!(paid <= expected_payouts[actor]);
            if self.deleted[actor] {
                assert_eq!(paid, expected_payouts[actor]);
                assert!(env
                    .svm
                    .get_account(&a.portfolio)
                    .map_or(true, |account| account.lamports == 0
                        && account.data.is_empty()));
                continue;
            }
            let account = env.portfolio_state(a.portfolio);
            assert_eq!(account.owner, a.owner.pubkey().to_bytes());
            capital += account.capital.get();
            positive_pnl += account.pnl.get().max(0) as u128;
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert_eq!(actor, 0, "only the common holder owns a positive claim");
                assert_ne!(self.junior_face, 0);
                assert_eq!(receipt.terminal_positive_claim_face, self.junior_face);
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            let expected = match actor {
                1 if !self.settled[0] => ATTRIBUTION_DEPOSITS[actor],
                3 if !self.settled[1] => ATTRIBUTION_DEPOSITS[actor],
                _ => expected_payouts[actor],
            };
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get() + due as i128 + paid as i128,
                expected as i128,
                "actor {actor}: principal + original debts - prior payout"
            );
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
                .collect();
            let expected_legs: Vec<_> = (0..2)
                .filter(|&pair| {
                    (actor == 0 && self.pending[pair])
                        || (actor == DEBTORS[pair] && !self.settled[pair])
                })
                .collect();
            assert_eq!(legs.len(), expected_legs.len());
            for pair in expected_legs {
                let leg = legs
                    .iter()
                    .find(|leg| leg.asset_index as usize == pair + 1)
                    .unwrap();
                let q = LOTS[pair] as i128 * POS_SCALE as i128 * self.sign;
                assert_eq!(
                    leg.side,
                    if (actor == 0) == (self.sign > 0) {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(leg.basis_pos_q, if actor == 0 { 0 } else { -q });
                assert_eq!(leg.loss_weight, LOTS[pair] * POS_SCALE);
            }
            let close = close_progress(&account);
            crate::support::fuzz_model::verify_close_residual_partition(
                "common pending holder",
                &close,
            )
            .unwrap();
            assert_eq!(
                close,
                CloseProgressLedgerV16::EMPTY,
                "solvent pending weight is not a close-residual credit"
            );
        }
        assert_eq!(group.c_tot, capital);
        assert_eq!(group.pnl_pos_tot, positive_pnl);
        assert_eq!(
            group.materialized_portfolio_count,
            self.deleted.iter().filter(|deleted| !**deleted).count() as u64
        );
        assert_eq!(group.insurance, 0);
        for pair in 0..2 {
            let asset = group.assets[pair + 1];
            let holder_side = usize::from(self.sign < 0);
            let mut oi = [0; 2];
            let mut weights = [0; 2];
            let mut stored = [0; 2];
            let mut pending = [0; 2];
            if self.pending[pair] {
                weights[holder_side] = LOTS[pair] * POS_SCALE;
                stored[holder_side] = 1;
                pending[holder_side] = 1;
            }
            if !self.settled[pair] {
                oi[1 - holder_side] = LOTS[pair] * POS_SCALE;
                weights[1 - holder_side] = oi[1 - holder_side];
                stored[1 - holder_side] = 1;
            }
            assert_eq!([asset.a_long, asset.a_short], [ADL_ONE; 2]);
            assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], oi);
            assert_eq!(
                [asset.loss_weight_sum_long, asset.loss_weight_sum_short],
                weights
            );
            assert_eq!(
                [asset.stored_pos_count_long, asset.stored_pos_count_short],
                stored
            );
            assert_eq!(
                [
                    asset.pending_obligation_count_long,
                    asset.pending_obligation_count_short
                ],
                pending
            );
        }
        if self.settled.contains(&false) {
            let holder = env.portfolio_state(world.actors[0].portfolio);
            assert_eq!(holder.pnl.get(), debt.iter().sum::<u128>() as i128);
            assert_eq!(holder.capital.get(), ATTRIBUTION_DEPOSITS[0]);
            assert!(!resolved_receipt(&holder).present);
            assert!(!group.payout_snapshot_captured);
            assert_eq!(env.token_amount(world.actors[0].token), 0);
        }
        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
        let custody = group.vault
            + world
                .actors
                .iter()
                .map(|a| u128::from(env.token_amount(a.token)))
                .sum::<u128>();
        assert_eq!(custody, ATTRIBUTION_DEPOSITS.iter().sum());
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), custody);
    }

    fn delete(&mut self, world: &mut AttributionWorld, actor: usize) {
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
        let rent = world
            .env
            .svm
            .get_account(&world.actors[actor].portfolio)
            .unwrap()
            .lamports;
        let market = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        let group = world.env.market_state().1;
        land(world, &[Action::Delete(actor)], None);
        self.deleted[actor] = true;
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market + rent
        );
        let next = world.env.market_state().1;
        assert_eq!(next.assets, group.assets);
        assert_eq!(next.source_credit, group.source_credit);
        assert_eq!(next.resolved_payout_ledger, group.resolved_payout_ledger);
        self.check(world);
    }
}

#[test]
fn v16_program_shared_holder_pending_domains_survive_partial_detach_and_debtor_close_orders() {
    let mut peak = 0;
    for reverse_sides in [false, true] {
        for first in 0..2 {
            for early_settlement in [false, true] {
                for detach_first in [false, true] {
                    println!("common holder: reversed={reverse_sides}, first={first}, early={early_settlement}, detach_first={detach_first}");
                    let mut world = AttributionWorld::new(reverse_sides);
                    let mut model = Model {
                        sign: if reverse_sides { -1 } else { 1 },
                        junior_face: if early_settlement { debts()[first] } else { 0 },
                        pending: [true; 2],
                        settled: [false; 2],
                        deleted: [false; 5],
                    };
                    for pair in 0..2 {
                        let holder = &world.actors[0];
                        let debtor = &world.actors[DEBTORS[pair]];
                        world.env.trade_asset_with_cu(
                            (pair + 1) as u16,
                            &holder.owner,
                            holder.portfolio,
                            &debtor.owner,
                            debtor.portfolio,
                            LOTS[pair] as i128 * POS_SCALE as i128 * model.sign,
                            1_000_000,
                            0,
                        );
                        let asset = world.env.market_state().1.assets[pair + 1];
                        assert_eq!(
                            [asset.oi_eff_long_q, asset.oi_eff_short_q],
                            [LOTS[pair] * POS_SCALE; 2]
                        );
                    }
                    let debtors_before: Vec<_> = DEBTORS
                        .iter()
                        .map(|&actor| world.env.svm.get_account(&world.actors[actor].portfolio))
                        .collect();
                    world.env.svm.warp_to_slot(20);
                    for pair in 0..2 {
                        world.env.push_auth_mark_for_asset_as_admin(
                            (pair + 1) as u16,
                            20,
                            (1_000_000 + MOVES[pair] as i128 * model.sign) as u64,
                        );
                    }
                    for actor in [4, 0] {
                        world.env.crank(
                            world.actors[actor].portfolio,
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 20,
                                observations: crank_observations_for_assets(&[1, 2]),
                            },
                        );
                    }
                    for pair in 0..2 {
                        world.env.update_asset_lifecycle_as_admin_with_cu(
                            processor::ASSET_ACTION_SHUTDOWN,
                            (pair + 1) as u16,
                            20,
                            0,
                        );
                        let holder = &world.actors[0];
                        world.env.forfeit_recovery_leg_with_cu(
                            &holder.owner,
                            holder.portfolio,
                            (pair + 1) as u16,
                            u128::MAX,
                        );
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.actors[DEBTORS[pair]].portfolio),
                            debtors_before[pair]
                        );
                    }
                    model.check(&world);
                    peak = peak.max(land(
                        &mut world,
                        &[Action::Delete(0)],
                        Some((2, PercolatorError::EngineLockActive)),
                    ));
                    if early_settlement {
                        world.forfeit(DEBTORS[first]);
                        model.settled[first] = true;
                        model.check(&world);
                    }
                    let before = world.frame();
                    let assets = world.env.market_state().1.assets;
                    assert_cu_within("common-holder resolve", world.env.resolve(), CRANK_CU_LIMIT);
                    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
                    assert_eq!(world.env.market_state().1.assets, assets);
                    for (key, account) in before {
                        if key != world.env.market {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    model.check(&world);
                    world.env.svm.warp_to_slot(25);
                    peak = peak.max(land(
                        &mut world,
                        &[Action::Close(0), Action::Claim(0)],
                        Some((3, PercolatorError::EngineLockActive)),
                    ));
                    model.check(&world);
                    if detach_first {
                        land(&mut world, &[Action::Close(0)], None);
                        model.pending[0] = false;
                        model.check(&world);
                    }
                    let debtor = DEBTORS[first];
                    peak = peak.max(land(
                        &mut world,
                        &[
                            Action::Close(debtor),
                            Action::Delete(debtor),
                            Action::Claim(0),
                        ],
                        Some((4, PercolatorError::EngineLockActive)),
                    ));
                    model.check(&world);
                    land(&mut world, &[Action::Close(debtor)], None);
                    model.settled[first] = true;
                    model.check(&world);
                    model.delete(&mut world, debtor);
                    if !detach_first {
                        land(&mut world, &[Action::Close(0)], None);
                        model.pending[0] = false;
                        model.check(&world);
                    }
                    assert_eq!(model.pending, [false, true]);
                    peak = peak.max(land(
                        &mut world,
                        &[Action::Claim(0), Action::Close(0)],
                        Some((2, PercolatorError::EngineLockActive)),
                    ));
                    model.check(&world);
                    land(&mut world, &[Action::Close(0)], None);
                    model.pending[1] = false;
                    model.check(&world);
                    peak = peak.max(land(
                        &mut world,
                        &[Action::Close(0)],
                        Some((2, PercolatorError::EngineNonProgress)),
                    ));
                    let debtor = DEBTORS[1 - first];
                    land(&mut world, &[Action::Close(debtor)], None);
                    model.settled[1 - first] = true;
                    model.check(&world);
                    if detach_first {
                        model.delete(&mut world, debtor);
                    }
                    for _ in 0..4 {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[0].portfolio) {
                            break;
                        }
                        let before = world.frame();
                        land(&mut world, &[Action::Close(0)], None);
                        assert_ne!(world.frame(), before, "each bounded close must progress");
                        model.check(&world);
                    }
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[0].portfolio
                    ));
                    assert_eq!(world.env.token_amount(world.actors[0].token), 228_005);
                    assert_eq!(
                        world.env.market_state().1.payout_snapshot_captured,
                        early_settlement
                    );
                    let before_retry = world.frame();
                    // Live forfeit pays into junior liquidity; resolved settlement funds the
                    // source domain. Only the early debt needs a terminal junior receipt.
                    if early_settlement {
                        let ledger = world.env.market_state().1.resolved_payout_ledger;
                        assert_eq!(ledger.snapshot_residual, model.junior_face);
                        assert_eq!(
                            ledger.terminal_claim_exact_receipts_num,
                            model.junior_face * BOUND_SCALE
                        );
                        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                        land(&mut world, &[Action::Claim(0), Action::Claim(0)], None);
                    } else {
                        assert!(
                            !resolved_receipt(
                                &world.env.portfolio_state(world.actors[0].portfolio)
                            )
                            .present
                        );
                        peak = peak.max(land(
                            &mut world,
                            &[Action::Claim(0)],
                            Some((2, PercolatorError::EngineLockActive)),
                        ));
                    }
                    assert_eq!(
                        world.frame(),
                        before_retry,
                        "combined claim is paid exactly once"
                    );
                    model.check(&world);
                    if !detach_first {
                        model.delete(&mut world, debtor);
                    }
                    for actor in if first == 0 { [0, 2, 4] } else { [4, 2, 0] } {
                        if actor != 0 {
                            land(&mut world, &[Action::Close(actor)], None);
                            model.check(&world);
                        }
                        model.delete(&mut world, actor);
                    }
                    assert_eq!(world.env.market_state().1.vault, 0);
                    let env = &mut world.env;
                    let destination =
                        create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
                    let admin_before = env.svm.get_account(&env.admin.pubkey()).unwrap().lamports;
                    let rent_before = env.svm.get_account(&env.market).unwrap().lamports
                        + env.svm.get_account(&env.vault).unwrap().lamports;
                    let ix = ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    };
                    let cu = send_tx(
                        &mut env.svm,
                        env.program_id,
                        &env.payer,
                        ix,
                        vec![
                            AccountMeta::new(env.admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new(destination, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(env.mint, false),
                        ],
                        &[&env.admin],
                    )
                    .unwrap();
                    assert_cu_within("common-holder terminal slab", cu, CUSTODY_CU_LIMIT);
                    let tombstone = env.svm.get_account(&env.market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert!(env
                        .svm
                        .get_account(&env.vault)
                        .map_or(true, |account| account.lamports == 0
                            && account.data.is_empty()));
                    assert_eq!(
                        env.svm.get_account(&env.admin.pubkey()).unwrap().lamports,
                        admin_before + rent_before - tombstone.lamports
                    );
                    assert_eq!(env.token_amount(destination), 0);
                    for (actor, expected) in [228_005, 179_993, 300_000, 222_002, 777]
                        .into_iter()
                        .enumerate()
                    {
                        assert_eq!(env.token_amount(world.actors[actor].token), expected);
                    }
                }
            }
        }
    }
    println!("INV-039: 16 common-holder worlds, 88 exact rollbacks, 80 portfolio deletions, 16 slab retirements; peak rejected bundle CU={peak}");
}
