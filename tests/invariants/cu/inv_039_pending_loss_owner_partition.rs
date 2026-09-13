//! INV-039/048/066: portfolio deletion must not truncate a shared owner's entitlement.
//! Compare one portfolio with two/three publicly funded portfolios sharing an ATA.
//! Unlike the two-domain and shared-holder histories, this partitions one owner's
//! membership in one source domain while a different owner competes for that source.
//! Integral, solvent inputs isolate owner attribution from rounding and bankruptcy.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
    verify_close_residual_partition,
};

const DEPOSITS: [u128; 5] = [800_000, 720_000, 800_000, 720_000, 777];

struct Book {
    deposits: Vec<u128>,
    gains: Vec<i128>,
    quantities: Vec<i128>,
    basis: Vec<i128>,
    pending: Vec<bool>,
    paid: Vec<u128>,
    deleted: Vec<bool>,
}

impl Book {
    fn owner(actor: usize) -> usize {
        if actor >= 5 {
            0
        } else {
            actor
        }
    }

    fn check(&self, world: &AttributionWorld) {
        let group = world.env.market_state().1;
        let mut accounts = Vec::new();
        let mut oi = [0; 2];
        let mut weights = [0; 2];
        let mut stored = [0; 2];
        let mut pending = [0; 2];
        let mut owner_paid = [0u128; 5];
        for (i, actor) in world.actors.iter().enumerate() {
            let entitlement = self.deposits[i] as i128 + self.gains[i];
            owner_paid[Self::owner(i)] += self.paid[i];
            if self.deleted[i] {
                assert_eq!(self.paid[i] as i128, entitlement);
                assert!(world
                    .env
                    .svm
                    .get_account(&actor.portfolio)
                    .is_none_or(|a| { a.lamports == 0 && a.data.is_empty() }));
                continue;
            }
            let account = world.env.portfolio_state(actor.portfolio);
            assert_eq!(
                account.owner,
                world.actors[Self::owner(i)].owner.pubkey().to_bytes()
            );
            let receipt = resolved_receipt(&account);
            let unpaid_face = if receipt.present {
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            // Debtors are deliberately untouched until their own public settlement.
            let latent_debt = if self.basis[i] != 0 { self.gains[i] } else { 0 };
            assert_eq!(
                account.capital.get() as i128
                    + account.pnl.get()
                    + unpaid_face as i128
                    + self.paid[i] as i128
                    + latent_debt,
                entitlement,
                "portfolio {i}: its own input debt, not just a conserved owner total"
            );
            let legs: Vec<_> = account
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .filter(|l| l.active)
                .collect();
            let retained = self.basis[i] != 0 || self.pending[i];
            assert_eq!(legs.len(), usize::from(retained));
            if retained {
                let side = usize::from(self.quantities[i] < 0);
                let leg = legs[0];
                assert_eq!(leg.asset_index, 1);
                assert_eq!(
                    leg.side,
                    if side == 0 {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                assert_eq!(leg.basis_pos_q, self.basis[i]);
                assert_eq!(leg.loss_weight, self.quantities[i].unsigned_abs());
                oi[side] += self.basis[i].unsigned_abs();
                weights[side] += self.quantities[i].unsigned_abs();
                stored[side] += 1;
                pending[side] += u64::from(self.pending[i]);
            }
            let close = close_progress(&account);
            verify_close_residual_partition("INV-039 owner partition", &close).unwrap();
            assert_eq!(
                close,
                CloseProgressLedgerV16::default(),
                "solvent, not B coverage"
            );
            accounts.push(account);
        }
        for (owner, expected) in owner_paid.into_iter().enumerate() {
            // Each ATA is counted once even when several portfolios pay into it.
            assert_eq!(
                world.env.token_amount(world.actors[owner].token) as u128,
                expected
            );
        }
        let asset = group.assets[1];
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
        let vault = world.env.token_amount(world.env.vault) as u128;
        assert_eq!(
            vault + self.paid.iter().sum::<u128>(),
            DEPOSITS.iter().sum::<u128>()
        );
        assert_eq!(
            Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            DEPOSITS.iter().sum::<u128>()
        );
        let raw = world.env.svm.get_account(&world.env.market).unwrap();
        assert_market_stock_census(
            "INV-039 partition stocks",
            &group,
            &raw.data,
            &accounts,
            vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("INV-039 partition reservations", &group, &accounts)
            .unwrap();
    }

    fn close(&mut self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) {
        let before = world.frame();
        let token = world.actors[actor].token;
        let paid_before = world.env.token_amount(token);
        let cu = match world.payout(actor, false) {
            Ok(cu) => cu,
            Err(error) => {
                assert!(
                    is_engine_non_progress_error(&error),
                    "actor {actor}: {error}"
                );
                assert_eq!(world.frame(), before, "waiting close must roll back");
                self.check(world);
                return;
            }
        };
        assert_cu_within("INV-039 partition close", cu, CUSTODY_CU_LIMIT);
        *peak = (*peak).max(cu);
        self.basis[actor] = 0;
        self.pending[actor] = false;
        self.paid[actor] += u128::from(world.env.token_amount(token) - paid_before);
        for (key, account) in before {
            if ![
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                token,
            ]
            .contains(&key)
            {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "foreign Account {key}"
                );
            }
        }
        self.check(world);
    }

    fn delete(&mut self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) {
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
        let before = world.frame();
        let a = &world.actors[actor];
        *peak = (*peak).max(world.env.close_portfolio_with_cu(&a.owner, a.portfolio));
        for (key, account) in before {
            if ![world.env.market, a.portfolio].contains(&key) {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "deletion frame {key}"
                );
            }
        }
        self.deleted[actor] = true;
        self.check(world);
    }
}

fn partition(world: &mut AttributionWorld, lots: &[u128]) -> Vec<u128> {
    assert_eq!(lots.iter().sum::<u128>(), 4);
    let mut deposits = DEPOSITS.to_vec();
    deposits[0] = lots[0] * 200_000;
    for part in &lots[1..] {
        let amount = part * 200_000;
        let owner = Keypair::from_bytes(&world.actors[0].owner.to_bytes()).unwrap();
        let token = world.actors[0].token;
        let env = &mut world.env;
        env.send(
            env.withdraw_ix(world.actors[0].portfolio, amount),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(world.actors[0].portfolio, false),
                AccountMeta::new(token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap();
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            env.program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[&owner],
        )
        .unwrap();
        env.portfolios.push(portfolio.pubkey());
        env.send(
            env.deposit_ix(portfolio.pubkey(), amount),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
                AccountMeta::new(token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap();
        world.actors.push(AttributionActor {
            owner,
            portfolio: portfolio.pubkey(),
            token,
        });
        deposits.push(amount);
    }
    deposits
}

fn run(
    lots: &[u128],
    movement: u64,
    reverse: bool,
    early: bool,
    backwards: bool,
    peak: &mut u64,
    partial_deletions: &mut usize,
    receipt_retries: &mut usize,
) -> [u128; 5] {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            max_abs_funding_e9_per_slot: 0,
            liquidation_fee_bps: 0,
            ..production_risk_params()
        },
        DEPOSITS,
    );
    let deposits = partition(&mut world, lots);
    let sign = if reverse { -1 } else { 1 };
    let mut quantities = vec![lots[0] as i128, -4, 4, -4, 0];
    quantities.extend(lots[1..].iter().map(|q| *q as i128));
    for q in &mut quantities {
        *q *= POS_SCALE as i128 * sign;
    }
    let mut holders = vec![0];
    holders.extend(5..world.actors.len());
    holders.push(2);
    let sibling = [
        world.env.market_state().1.assets[0],
        world.env.market_state().1.assets[2],
    ];
    for &holder in &holders {
        let debtor = if holder == 2 { 3 } else { 1 };
        let cu = world.env.trade_asset_with_cu(
            1,
            &world.actors[holder].owner,
            world.actors[holder].portfolio,
            &world.actors[debtor].owner,
            world.actors[debtor].portfolio,
            quantities[holder],
            1_000_000,
            0,
        );
        *peak = (*peak).max(cu);
        assert_cu_within("INV-039 partition opening", cu, TRADE_CU_LIMIT);
    }
    let opening = world.env.market_state().1.assets[1];
    assert_eq!(
        [opening.oi_eff_long_q, opening.oi_eff_short_q],
        [8 * POS_SCALE; 2]
    );
    world.env.svm.warp_to_slot(20);
    world.env.push_auth_mark_for_asset_as_admin(
        1,
        20,
        (1_000_000 + movement as i128 * sign) as u64,
    );
    for &holder in &holders {
        *peak = (*peak).max(world.env.crank(
            world.actors[holder].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations(1),
            },
        ));
    }
    world
        .env
        .update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 1, 20, 0);
    for &holder in &holders {
        let a = &world.actors[holder];
        *peak = (*peak).max(world.env.forfeit_recovery_leg_with_cu(
            &a.owner,
            a.portfolio,
            1,
            u128::MAX,
        ));
    }
    let mut book = Book {
        deposits,
        gains: quantities
            .iter()
            .map(|q| q / POS_SCALE as i128 * sign * movement as i128)
            .collect(),
        basis: quantities.clone(),
        pending: vec![false; quantities.len()],
        paid: vec![0; quantities.len()],
        deleted: vec![false; quantities.len()],
        quantities,
    };
    for &holder in &holders {
        book.basis[holder] = 0;
        book.pending[holder] = true;
    }
    book.check(&world);
    if early {
        let a = &world.actors[1];
        *peak = (*peak).max(world.env.forfeit_recovery_leg_with_cu(
            &a.owner,
            a.portfolio,
            1,
            u128::MAX,
        ));
        book.basis[1] = 0;
        book.check(&world);
    }
    let before = world.frame();
    *peak = (*peak).max(world.env.resolve());
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    book.check(&world);
    world.env.svm.warp_to_slot(25);
    if backwards {
        holders.reverse();
    }
    book.close(&mut world, holders[0], peak);
    assert_eq!(
        world.env.token_amount(world.actors[holders[0]].token),
        0,
        "unbooked opposing debt cannot pay even the first portfolio sharing an ATA"
    );
    for debtor in if backwards { [3, 1] } else { [1, 3] } {
        book.close(&mut world, debtor, peak);
        book.delete(&mut world, debtor, peak);
    }
    book.close(&mut world, 4, peak);
    book.delete(&mut world, 4, peak);
    for _ in 0..4 {
        for &holder in &holders {
            if book.deleted[holder] {
                continue;
            }
            book.close(&mut world, holder, peak);
            if resolved_portfolio_is_terminal(&world.env, world.actors[holder].portfolio) {
                if resolved_receipt(&world.env.portfolio_state(world.actors[holder].portfolio))
                    .present
                {
                    let before = world.frame();
                    *peak = (*peak).max(world.payout(holder, true).expect("paid receipt retry"));
                    assert_eq!(world.frame(), before);
                    *receipt_retries += 1;
                }
                if Book::owner(holder) == 0
                    && holders.iter().any(|other| {
                        *other != holder
                            && Book::owner(*other) == 0
                            && (book.paid[*other] as i128)
                                < book.deposits[*other] as i128 + book.gains[*other]
                    })
                {
                    *partial_deletions += 1;
                }
                book.delete(&mut world, holder, peak);
            }
        }
    }
    assert!(book.deleted.iter().all(|deleted| *deleted));
    let group = world.env.market_state().1;
    assert_eq!([group.assets[0], group.assets[2]], sibling);
    assert_eq!(
        (
            group.vault,
            group.insurance,
            group.c_tot,
            group.pnl_pos_tot,
            group.materialized_portfolio_count
        ),
        (0, 0, 0, 0, 0)
    );
    let expected = [
        DEPOSITS[0] + 4 * movement as u128,
        DEPOSITS[1] - 4 * movement as u128,
        DEPOSITS[2] + 4 * movement as u128,
        DEPOSITS[3] - 4 * movement as u128,
        DEPOSITS[4],
    ];
    let paid =
        std::array::from_fn(|owner| world.env.token_amount(world.actors[owner].token) as u128);
    assert_eq!(paid, expected);
    paid
}

#[test]
fn v16_program_pending_owner_partition_preserves_shared_ata_entitlements_through_deletion() {
    let mut peak = 0;
    let mut partial_deletions = 0;
    let mut receipt_retries = 0;
    let mut worlds = 0;
    for movement in [8, 16_000] {
        let mut baseline = None;
        for reverse in [false, true] {
            for early in [false, true] {
                for backwards in [false, true] {
                    for lots in [&[4][..], &[1, 3][..], &[1, 1, 2][..]] {
                        let payout = run(
                            lots,
                            movement,
                            reverse,
                            early,
                            backwards,
                            &mut peak,
                            &mut partial_deletions,
                            &mut receipt_retries,
                        );
                        assert_eq!(*baseline.get_or_insert(payout), payout,
                            "lots={lots:?} movement={movement} reverse={reverse} early={early} backwards={backwards}");
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    assert!(
        partial_deletions > 0,
        "a deleted portfolio must leave its co-owner's separate entitlement unpaid"
    );
    assert!(
        receipt_retries > 0,
        "early settlement must exercise actual receipts"
    );
    println!("INV-039 owner partition: {worlds} worlds, {partial_deletions} deletions with co-owner value unpaid, {receipt_retries} receipt retries, peak {peak} CU");
}
