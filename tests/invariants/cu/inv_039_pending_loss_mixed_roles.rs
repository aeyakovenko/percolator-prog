//! INV-039: one pending creditor is also another domain's economic debtor.
//! Canceling its local positive PnL against its loss must preserve the original
//! payer's debt and the downstream holder's value through resolved close order.

use super::*;

const HOLDERS: [usize; 2] = [0, 2];
const PAYERS: [usize; 2] = [1, 0];

struct MixedModel {
    sign: i128,
    debt: [u128; 2],
    pending: [bool; 2],
    booked: [bool; 2],
    detached: [bool; 2],
    deleted: [bool; 5],
}

impl MixedModel {
    fn endpoint(&self, actor: usize) -> u128 {
        let mut amount = ATTRIBUTION_DEPOSITS[actor];
        for pair in 0..2 {
            if actor == HOLDERS[pair] {
                amount += self.debt[pair];
            }
            if actor == PAYERS[pair] {
                amount -= self.debt[pair];
            }
        }
        amount
    }

    fn check(&self, world: &AttributionWorld) {
        let env = &world.env;
        let group = env.market_state().1;
        let mut portfolios = Vec::new();
        for (actor, a) in world.actors.iter().enumerate() {
            let paid = u128::from(env.token_amount(a.token));
            assert!(paid <= self.endpoint(actor), "owner {actor}: payout bound");
            if self.deleted[actor] {
                assert_eq!(paid, self.endpoint(actor));
                assert!(env.svm.get_account(&a.portfolio).map_or(true, |account| {
                    account.lamports == 0 && account.data.is_empty()
                }));
                continue;
            }
            let account = env.portfolio_state(a.portfolio);
            assert_eq!(account.owner, a.owner.pubkey().to_bytes());
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            let unbooked = (0..2)
                .filter(|&pair| actor == PAYERS[pair] && !self.booked[pair])
                .map(|pair| self.debt[pair])
                .sum::<u128>();
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get() + due as i128 + paid as i128,
                (self.endpoint(actor) + unbooked) as i128,
                "owner {actor}: original credit and debt remain attributed; booked={:?}, detached={:?}, capital={}, pnl={}, paid={paid}, receipt={receipt:?}",
                self.booked, self.detached, account.capital.get(), account.pnl.get()
            );
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
                .collect();
            let mut expected_legs = 0;
            for pair in 0..2 {
                let holder = actor == HOLDERS[pair] && self.pending[pair];
                let payer = actor == PAYERS[pair] && !self.detached[pair];
                if !holder && !payer {
                    continue;
                }
                expected_legs += 1;
                let leg = legs
                    .iter()
                    .find(|leg| leg.asset_index as usize == pair + 1)
                    .unwrap();
                assert_eq!(
                    leg.side,
                    if holder == (self.sign > 0) {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(
                    leg.basis_pos_q,
                    if holder {
                        0
                    } else {
                        -self.sign * POS_SCALE as i128
                    }
                );
                assert_eq!(leg.loss_weight, POS_SCALE);
            }
            assert_eq!(
                legs.len(),
                expected_legs,
                "owner {actor}: exact domain ownership"
            );
            assert_eq!(close_progress(&account), CloseProgressLedgerV16::EMPTY);
            portfolios.push(account);
        }
        for pair in 0..2 {
            let asset = group.assets[pair + 1];
            let side = usize::from(self.sign < 0);
            let mut oi = [0; 2];
            let mut weight = [0; 2];
            let mut count = [0; 2];
            let mut pending = [0; 2];
            if self.pending[pair] {
                weight[side] = POS_SCALE;
                count[side] = 1;
                pending[side] = 1;
            }
            if !self.detached[pair] {
                oi[1 - side] = POS_SCALE;
                weight[1 - side] = POS_SCALE;
                count[1 - side] = 1;
            }
            assert_eq!([asset.a_long, asset.a_short], [ADL_ONE; 2]);
            assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], oi);
            assert_eq!(
                [asset.loss_weight_sum_long, asset.loss_weight_sum_short],
                weight
            );
            assert_eq!(
                [asset.stored_pos_count_long, asset.stored_pos_count_short],
                count
            );
            assert_eq!(
                [
                    asset.pending_obligation_count_long,
                    asset.pending_obligation_count_short
                ],
                pending
            );
        }
        assert_eq!(group.insurance, 0);
        assert_eq!(group.materialized_portfolio_count, portfolios.len() as u64);
        let custody = u128::from(env.token_amount(env.vault));
        crate::support::fuzz_model::assert_market_stock_census(
            "mixed pending creditor/debtor",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &portfolios,
            custody,
        )
        .unwrap();
        let supply = ATTRIBUTION_DEPOSITS.iter().sum::<u128>();
        assert_eq!(
            custody
                + world
                    .actors
                    .iter()
                    .map(|a| u128::from(env.token_amount(a.token)))
                    .sum::<u128>(),
            supply
        );
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), supply);
        assert_eq!(
            mint.mint_authority,
            solana_program::program_option::COption::None
        );
    }

    fn close(&mut self, world: &mut AttributionWorld, actor: usize) {
        land(world, &[Action::Close(actor)], None);
        if actor == 0 {
            // Settlement books both local legs before one canonical leg detaches.
            self.booked[1] = true;
            if self.pending[0] {
                self.pending[0] = false;
            } else {
                self.detached[1] = true;
            }
        } else if actor == 1 {
            self.booked[0] = true;
            self.detached[0] = true;
        } else if actor == 2 {
            self.pending[1] = false;
        }
        self.check(world);
    }

    fn delete(&mut self, world: &mut AttributionWorld, actor: usize) {
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
        let before = world.env.market_state().1;
        let rent = world
            .env
            .svm
            .get_account(&world.actors[actor].portfolio)
            .unwrap()
            .lamports;
        let market_lamports = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        land(world, &[Action::Delete(actor)], None);
        self.deleted[actor] = true;
        let after = world.env.market_state().1;
        assert_eq!(after.assets, before.assets);
        assert_eq!(after.source_credit, before.source_credit);
        assert_eq!(after.resolved_payout_ledger, before.resolved_payout_ledger);
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market_lamports + rent
        );
        self.check(world);
    }
}

fn prepare() -> (AttributionWorld, MixedModel) {
    let mut world = AttributionWorld::new(false);
    let model = MixedModel {
        sign: 1,
        debt: [30_000, 40_000],
        pending: [true; 2],
        booked: [false; 2],
        detached: [false; 2],
        deleted: [false; 5],
    };
    send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &world.env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &world.env.admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&world.env.admin],
    )
    .unwrap();
    // Stage the first credit before opening the shared owner's losing leg. No
    // public refresh of that owner can prematurely book the second price move.
    for pair in 0..2 {
        let asset = (pair + 1) as u16;
        let holder = &world.actors[HOLDERS[pair]];
        let payer = &world.actors[PAYERS[pair]];
        let cu = world.env.trade_asset_with_cu(
            asset,
            &holder.owner,
            holder.portfolio,
            &payer.owner,
            payer.portfolio,
            model.sign * POS_SCALE as i128,
            1_000_000,
            0,
        );
        assert_cu_within("mixed-role opening", cu, TRADE_CU_LIMIT);
        let a = world.env.market_state().1.assets[pair + 1];
        assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], [POS_SCALE; 2]);
        let payer_before = world.env.svm.get_account(&payer.portfolio);
        let slot = 20 + pair as u64;
        world.env.svm.warp_to_slot(slot);
        world.env.push_auth_mark_for_asset_as_admin(
            asset,
            slot,
            (1_000_000 + model.sign * model.debt[pair] as i128) as u64,
        );
        for actor in [4, HOLDERS[pair]] {
            world.env.crank(
                world.actors[actor].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(asset),
                },
            );
        }
        world.env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            asset,
            slot,
            0,
        );
        let holder = &world.actors[HOLDERS[pair]];
        world
            .env
            .forfeit_recovery_leg_with_cu(&holder.owner, holder.portfolio, asset, u128::MAX);
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.actors[PAYERS[pair]].portfolio),
            payer_before
        );
    }
    model.check(&world);
    let before = world.frame();
    let assets = world.env.market_state().1.assets;
    assert_cu_within("mixed-role resolution", world.env.resolve(), CRANK_CU_LIMIT);
    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
    assert_eq!(world.env.market_state().1.assets, assets);
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    world.env.svm.warp_to_slot(26);
    model.check(&world);
    (world, model)
}

fn reject_suffix(world: &mut AttributionWorld, actions: &[Action]) -> u64 {
    world.env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend(actions.iter().map(|action| action.instruction(world)));
    let sender = world.actors[3].owner.pubkey();
    ixs.push(system_instruction::transfer(
        &sender,
        &world.env.admin.pubkey(),
        world.env.svm.get_account(&sender).unwrap().lamports + 1,
    ));
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer, &world.actors[3].owner],
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .into_iter()
        .map(|key| (key, world.env.svm.get_account(&key)))
        .collect();
    let failure = world
        .env
        .svm
        .send_transaction(tx)
        .expect_err("insufficient SOL suffix after successful closes");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError((2 + actions.len()) as u8, InstructionError::Custom(1))
    );
    for (key, mut account) in before {
        if key == world.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            world.env.svm.get_account(&key),
            account,
            "complete rollback: {key}"
        );
    }
    assert_cu_within(
        "mixed-role close suffix",
        failure.meta.compute_units_consumed,
        500_000,
    );
    failure.meta.compute_units_consumed
}

#[test]
fn v16_program_mixed_creditor_debtor_preserves_pending_attribution_through_resolved_close_order() {
    let mut peak = 0;
    for original_payer_first in [true, false] {
        println!("mixed roles: original_payer_first={original_payer_first}");
        let (mut world, mut model) = prepare();
        let original_payer = world.env.svm.get_account(&world.actors[1].portfolio);
        if original_payer_first {
            model.close(&mut world, 1);
            model.delete(&mut world, 1);
        }
        peak = peak.max(reject_suffix(
            &mut world,
            &[Action::Close(0), Action::Close(0)],
        ));
        model.check(&world);
        model.close(&mut world, 0);
        assert_eq!(model.pending, [false, true]);
        assert_eq!(model.booked, [original_payer_first, true]);
        assert_eq!(model.detached, [original_payer_first, false]);
        model.close(&mut world, 0);
        if !original_payer_first {
            assert_eq!(
                world.env.svm.get_account(&world.actors[1].portfolio),
                original_payer
            );
            // The original creditor can become a net debtor and finish,
            // but that cannot erase its still-absent payer's old debt.
            model.delete(&mut world, 0);
            peak = peak.max(reject_suffix(
                &mut world,
                &[Action::Close(2), Action::Close(1)],
            ));
            model.check(&world);
            model.close(&mut world, 2);
            model.close(&mut world, 1);
            model.delete(&mut world, 1);
        }
        let order = if original_payer_first {
            [2, 0, 3, 4]
        } else {
            [0, 2, 4, 3]
        };
        for actor in order {
            if model.deleted[actor] {
                continue;
            }
            for _ in 0..4 {
                if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                    break;
                }
                let before = world.frame();
                model.close(&mut world, actor);
                assert_ne!(world.frame(), before, "bounded continuation progresses");
            }
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[actor].token)),
                model.endpoint(actor)
            );
            model.delete(&mut world, actor);
        }
        assert_eq!(world.env.market_state().1.vault, 0);
        assert_eq!(model.deleted, [true; 5]);
        println!("mixed-role order completed: original_payer_first={original_payer_first}");
    }
    println!("INV-039 mixed roles: 2 close orders, 3 exact suffix rollbacks, 10 exact owner payouts and deletions; peak suffix CU={peak}");
}
