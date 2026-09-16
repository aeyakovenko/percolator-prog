//! INV-024/031/039/048/066/073: mixed-role ADL histories enter an underfunded
//! receipt while another owner's senior capital remains unpaid. The suffix
//! oracle starts at that public checkpoint and partitions source realization,
//! receipt payments, expiry and terminal insurance recredit. It is not a full
//! input-derived oracle for the preceding liquidation history.

use super::*;
use crate::support::reference_math::mul_div_ceil;

const INPUTS: [u128; 5] = [1_000, 250, 1_000, 250, 777];
const PROVIDER: u128 = 751;
const INSURED: u128 = 123;
const BACKING_EXPIRY: u64 = 25;

#[derive(Default)]
struct Evidence {
    peak: u64,
    rollbacks: usize,
    waits: usize,
    retries: usize,
    recredit: usize,
}

fn trade(world: &mut AttributionWorld, asset: u16, holder: usize, debtor: usize, q: i128) {
    world.env.trade_asset_with_cu(
        asset,
        &world.actors[holder].owner,
        world.actors[holder].portfolio,
        &world.actors[debtor].owner,
        world.actors[debtor].portfolio,
        q,
        100,
        0,
    );
}

fn fund(world: &mut AttributionWorld) -> Pubkey {
    let env = &mut world.env;
    let source = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &source,
            &env.admin.pubkey(),
            &[],
            (PROVIDER + INSURED) as u64,
        )
        .unwrap(),
        &[&env.admin],
    )
    .unwrap();
    for (domain, amount, expiry_slot) in [(5, 1, 12), (3, 750, BACKING_EXPIRY)] {
        let asset = domain / 2;
        env.send(
            ProgInstruction::TopUpBackingBucket {
                domain,
                market_id: env.asset_market_id(asset),
                authority_epoch: env.control_sequences(asset as usize).authority_epoch,
                intent_id: next_control_sequence(
                    env.control_sequences(asset as usize).backing_top_up,
                ),
                backing_fee_bps: 0,
                insurance_share_bps: 0,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&env.admin.insecure_clone()],
        )
        .unwrap();
    }
    source
}

fn accounts(world: &AttributionWorld) -> [PortfolioAccountV16; 5] {
    std::array::from_fn(|i| world.env.portfolio_state(world.actors[i].portfolio))
}

fn paid(world: &AttributionWorld) -> [u128; 5] {
    std::array::from_fn(|i| world.env.token_amount(world.actors[i].token) as u128)
}

fn census(world: &AttributionWorld, source: Pubkey, deleted: bool) {
    let env = &world.env;
    let g = env.market_state().1;
    let portfolios = if deleted {
        vec![]
    } else {
        accounts(world).to_vec()
    };
    let vault = env.token_amount(env.vault) as u128;
    let supply = INPUTS.iter().sum::<u128>() + PROVIDER + INSURED;
    assert_eq!(g.vault, vault);
    assert_eq!(
        vault + paid(world).iter().sum::<u128>() + env.token_amount(source) as u128,
        supply
    );
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        supply
    );
    assert_market_stock_census(
        "mixed underfunded receipt",
        &g,
        &env.svm.get_account(&env.market).unwrap().data,
        &portfolios,
        vault,
    )
    .unwrap();
    assert_reservation_encumbrance_census("mixed underfunded receipt", &g, &portfolios).unwrap();
    let mut oi = [[0u128; 2]; 3];
    let mut weights = oi;
    let mut stored = [[0u64; 2]; 3];
    let mut pending = stored;
    for p in portfolios {
        verify_close_residual_partition("mixed underfunded receipt", &close_progress(&p)).unwrap();
        for l in p
            .legs
            .iter()
            .map(|l| l.try_to_runtime().unwrap())
            .filter(|l| l.active)
        {
            let index = l.asset_index as usize;
            let side = usize::from(l.side == SideV16::Short);
            let a = g.assets[index];
            let (current_a, epoch) = if side == 0 {
                (a.a_long, a.epoch_long)
            } else {
                (a.a_short, a.epoch_short)
            };
            let q = if l.epoch_snap == epoch {
                mul_div_ceil(l.basis_pos_q.unsigned_abs(), current_a, l.a_basis).unwrap()
            } else {
                assert_eq!(l.epoch_snap + 1, epoch);
                assert_eq!(
                    if side == 0 { a.mode_long } else { a.mode_short },
                    percolator::SideModeV16::ResetPending
                );
                0
            };
            oi[index][side] += q;
            if l.epoch_snap == epoch {
                weights[index][side] += l.loss_weight;
            }
            stored[index][side] += 1;
            pending[index][side] += u64::from(l.basis_pos_q == 0 && l.loss_weight != 0);
        }
    }
    for i in 0..3 {
        let a = g.assets[i];
        assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[i]);
        assert_eq!(
            [a.loss_weight_sum_long, a.loss_weight_sum_short],
            weights[i]
        );
        assert_eq!(
            [a.stored_pos_count_long, a.stored_pos_count_short],
            stored[i]
        );
        assert_eq!(
            [
                a.pending_obligation_count_long,
                a.pending_obligation_count_short
            ],
            pending[i]
        );
    }
}

fn atomic(world: &mut AttributionWorld, ix: Instruction, changed: &[Pubkey], e: &mut Evidence) {
    e.peak = e.peak.max(land(
        world,
        &[ix.clone(), deletion(world, 4, false)],
        &[],
        &[],
        Some(1),
    ));
    e.rollbacks += 1;
    e.peak = e.peak.max(land(world, &[ix], &[], changed, None));
}

fn setup(lots: i128, e: &mut Evidence) -> (AttributionWorld, Pubkey) {
    let mut world = AttributionWorld::new_with_deposits(
        false,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 100,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_abs_funding_e9_per_slot: 0,
            liquidation_fee_bps: 0,
            ..V16CuMarketParams::default()
        },
        INPUTS,
    );
    let source = fund(&mut world);
    // Lower-index debt realizes first, preserving the peer's backed asset-1
    // claim until after the mixed owner's partial receipt is created.
    trade(&mut world, 2, 0, 1, 20 * POS_SCALE as i128);
    trade(&mut world, 1, 2, 3, 20 * POS_SCALE as i128);
    trade(&mut world, 0, 2, 0, lots * POS_SCALE as i128);
    for (offset, mark) in (105..=150).step_by(5).enumerate() {
        let slot = 2 + offset as u64;
        world.env.svm.warp_to_slot(slot);
        for asset in 0..3 {
            world
                .env
                .push_auth_mark_for_asset_as_admin(asset, slot, mark);
        }
        for actor in [1, 0, 3, 2] {
            if let Some(cu) = world.env.crank_if_actionable(
                world.actors[actor].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations_for_assets(&[0, 1, 2]),
                },
            ) {
                e.peak = e.peak.max(cu);
            }
            census(&world, source, false);
        }
        if slot == 2 {
            for _ in 0..16 {
                match world.env.crank_if_actionable(
                    world.actors[1].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(2),
                    },
                ) {
                    Some(cu) => e.peak = e.peak.max(cu),
                    None => break,
                }
                census(&world, source, false);
            }
            let a = world.env.market_state().1.assets[2];
            assert!(0 < a.a_long && a.a_long < ADL_ONE);
            assert!(0 < a.oi_eff_long_q && a.oi_eff_long_q < 20 * POS_SCALE);
        }
    }
    let mixed = world.env.portfolio_state(world.actors[0].portfolio);
    let g = world.env.market_state().1;
    let legs: Vec<_> = mixed
        .legs
        .iter()
        .map(|l| l.try_to_runtime().unwrap())
        .filter(|l| l.active)
        .collect();
    assert_eq!(legs.len(), 2);
    assert!(legs
        .iter()
        .any(|l| l.asset_index == 0 && l.side == SideV16::Short && l.k_snap < 0));
    assert!(legs.iter().any(|l| l.asset_index == 2
        && l.side == SideV16::Long
        && l.k_snap > 0
        && l.a_basis > g.assets[2].a_long));
    assert!(mixed.pnl.get() > 0);
    assert_eq!(mixed.capital.get(), INPUTS[0] - 5 * lots as u128);
    assert_eq!(
        world
            .env
            .portfolio_state(world.actors[2].portfolio)
            .pnl
            .get(),
        1_000 + 50 * lots
    );
    let env = &mut world.env;
    env.send(
        ProgInstruction::TopUpInsuranceDomain {
            domain: 2,
            market_id: env.asset_market_id(1),
            authority_epoch: env.control_sequences(1).authority_epoch,
            intent_id: next_control_sequence(env.control_sequences(1).insurance_top_up),
            amount: INSURED,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin.insecure_clone()],
    )
    .unwrap();
    for _ in 0..16 {
        match world.env.crank_if_actionable(
            world.actors[3].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 11,
                observations: crank_observations(1),
            },
        ) {
            Some(cu) => e.peak = e.peak.max(cu),
            None => break,
        }
        census(&world, source, false);
    }
    let close = close_progress(&world.env.portfolio_state(world.actors[3].portfolio));
    assert!(close.active && close.finalized);
    assert_eq!(
        (
            close.gross_loss_at_close_start,
            close.insurance_spent,
            close.b_loss_booked
        ),
        (750, INSURED, 750 - INSURED)
    );
    assert_eq!(
        world.env.market_state().1.insurance_domain_spent[2],
        INSURED
    );
    world.env.resolve();
    world.env.svm.warp_to_slot(16);
    for actor in [1, 3] {
        for _ in 0..8 {
            if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                break;
            }
            e.peak = e.peak.max(world.payout(actor, false).unwrap());
            census(&world, source, false);
        }
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
    }
    e.peak = e.peak.max(world.payout(2, false).unwrap());
    for _ in 0..16 {
        if resolved_receipt(&world.env.portfolio_state(world.actors[0].portfolio)).present {
            break;
        }
        let before = world.frame();
        match world.payout(0, false) {
            Ok(cu) => {
                e.peak = e.peak.max(cu);
                assert_ne!(world.frame(), before);
            }
            Err(error) => {
                assert!(is_engine_non_progress_error(&error), "{error}");
                assert_eq!(world.frame(), before);
                e.waits += 1;
                e.peak = e.peak.max(world.payout(2, false).unwrap());
            }
        }
        census(&world, source, false);
    }
    (world, source)
}

fn fresh(g: &MarketGroupV16) -> u128 {
    let n: u128 = g
        .source_credit
        .iter()
        .map(|s| s.fresh_reserved_backing_num)
        .sum();
    assert_eq!(n % BOUND_SCALE, 0);
    n / BOUND_SCALE
}

struct ReceiptBook {
    senior: [u128; 5],
    faces: [u128; 5],
    capital_paid: [u128; 5],
    junior_paid: [u128; 5],
    created: [bool; 5],
    budget: u128,
}

impl ReceiptBook {
    fn new(world: &AttributionWorld, lots: i128) -> Self {
        let p = accounts(world);
        let g = world.env.market_state().1;
        let r = resolved_receipt(&p[0]);
        assert!(
            r.present
                && !r.finalized
                && 0 < r.paid_effective
                && r.paid_effective < r.terminal_positive_claim_face
        );
        assert_eq!(p[4].capital.get(), INPUTS[4]);
        assert_eq!(p[2].capital.get(), INPUTS[2] + 5 * lots as u128);
        assert!(p[2]
            .legs
            .iter()
            .all(|l| !l.try_to_runtime().unwrap().active));
        assert_eq!(
            p[2].source_domains
                .iter()
                .filter(|s| s.is_occupied())
                .count(),
            1
        );
        assert!(p[2]
            .source_domains
            .iter()
            .any(|s| s.is_occupied() && s.domain.get() == 3));
        assert_eq!(g.source_backing_buckets[3].expiry_slot, BACKING_EXPIRY);
        assert_eq!(
            g.source_credit[3].fresh_reserved_backing_num,
            1_000 * BOUND_SCALE
        );
        let senior = [
            INPUTS[0] - 5 * lots as u128,
            0,
            p[2].capital.get(),
            0,
            INPUTS[4],
        ];
        assert_eq!(paid(world), [senior[0] + r.paid_effective, 0, 0, 0, 0]);
        let book = Self {
            senior,
            faces: [
                r.terminal_positive_claim_face,
                0,
                p[2].pnl.get() as u128,
                0,
                0,
            ],
            capital_paid: [senior[0], 0, 0, 0, 0],
            junior_paid: [r.paid_effective, 0, 0, 0, 0],
            created: [true, false, false, false, false],
            budget: g.vault + r.paid_effective
                - g.c_tot
                - g.insurance
                - fresh(&g)
                - g.backing_provider_earnings_total,
        };
        book.check(world);
        book
    }

    fn gross(&self, actor: usize) -> u128 {
        let total: u128 = self.faces.iter().sum();
        if total == 0 {
            0
        } else {
            self.faces[actor] * self.budget.min(total) / total
        }
    }

    fn expected_paid(&self) -> [u128; 5] {
        std::array::from_fn(|i| self.capital_paid[i] + self.junior_paid[i])
    }

    fn check(&self, world: &AttributionWorld) {
        let p = accounts(world);
        let g = world.env.market_state().1;
        let ledger = g.resolved_payout_ledger;
        assert_eq!(
            paid(world),
            self.expected_paid(),
            "owner-local payout, not merely conserved custody"
        );
        let mut wrong_owner = paid(world);
        wrong_owner[0] -= 1;
        wrong_owner[4] += 1;
        assert_eq!(
            wrong_owner.iter().sum::<u128>(),
            paid(world).iter().sum::<u128>()
        );
        assert_ne!(wrong_owner, self.expected_paid());
        for i in 0..5 {
            assert_eq!(
                p[i].capital.get(),
                self.senior[i] - self.capital_paid[i],
                "senior owner {i}"
            );
            assert_eq!(
                p[i].pnl.get(),
                if self.created[i] {
                    0
                } else {
                    self.faces[i] as i128
                }
            );
            let r = resolved_receipt(&p[i]);
            if r.present {
                assert_eq!(r.terminal_positive_claim_face, self.faces[i]);
                assert_eq!(r.prior_bound_contribution_num, self.faces[i] * BOUND_SCALE);
                assert_eq!(r.paid_effective, self.junior_paid[i]);
            } else if self.created[i] && self.faces[i] != 0 {
                assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                assert_eq!(self.junior_paid[i], self.gross(i));
            }
        }
        let exact: u128 = (0..5)
            .filter(|i| self.created[*i])
            .map(|i| self.faces[i])
            .sum();
        let bound: u128 = self.faces.iter().sum();
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            exact * BOUND_SCALE
        );
        assert_eq!(
            ledger.terminal_claim_bound_unreceipted_num,
            (bound - exact) * BOUND_SCALE
        );
        assert_eq!(ledger.snapshot_residual, self.budget);
        assert_eq!(
            ledger.current_payout_rate_num * bound,
            self.budget.min(bound) * ledger.current_payout_rate_den
        );
        assert_eq!(
            g.vault + self.junior_paid.iter().sum::<u128>(),
            self.budget + g.c_tot + g.insurance + fresh(&g) + g.backing_provider_earnings_total
        );
        assert_eq!(g.insurance, 0);
        assert_eq!(g.insurance_domain_spent[2], INSURED);
        assert!(!ledger.payout_halted);
    }

    fn step(
        &mut self,
        world: &mut AttributionWorld,
        actor: usize,
        claim: bool,
        source: Pubkey,
        e: &mut Evidence,
    ) {
        let before = accounts(world);
        let old = world.env.market_state().1;
        let ix = payout(world, actor, claim);
        let changed = [
            world.env.market,
            world.env.vault,
            world.actors[actor].portfolio,
            world.actors[actor].token,
        ];
        atomic(world, ix, &changed, e);
        let after = accounts(world);
        let new = world.env.market_state().1;
        let mut converted = 0;
        for s in before[actor]
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
        {
            if after[actor]
                .source_domains
                .iter()
                .any(|a| a.is_occupied() && a.domain == s.domain)
            {
                continue;
            }
            let c = old.source_credit[s.domain.get() as usize];
            assert_eq!(
                c.valid_liened_backing_num
                    + c.impaired_liened_backing_num
                    + c.insurance_credit_reserved_num,
                0
            );
            let rate = percolator::CREDIT_RATE_SCALE.min(
                c.fresh_reserved_backing_num * percolator::CREDIT_RATE_SCALE
                    / c.positive_claim_bound_num,
            );
            let released = before[actor].pnl.get() as u128 - before[actor].reserved_pnl.get();
            let amount = released.min(s.source_claim_bound_num.get() / BOUND_SCALE) * rate
                / percolator::CREDIT_RATE_SCALE;
            let next = new.source_credit[s.domain.get() as usize];
            assert_eq!(
                next.spent_backing_num,
                c.spent_backing_num + amount * BOUND_SCALE
            );
            assert_eq!(
                next.provider_receivable_num,
                c.provider_receivable_num + amount * BOUND_SCALE
            );
            converted += amount;
        }
        self.senior[actor] += converted;
        self.faces[actor] -= converted;
        let expired: u128 = old
            .source_backing_buckets
            .iter()
            .zip(&new.source_backing_buckets)
            .filter(|(a, b)| {
                a.status == percolator::BackingBucketStatusV16::Fresh && b.status != a.status
            })
            .map(|(a, _)| a.fresh_unliened_backing_num / BOUND_SCALE)
            .sum();
        assert_eq!(fresh(&old) - fresh(&new), converted + expired);
        self.budget += expired;
        if after[actor].capital.get() == 0 && after[actor].pnl.get() == 0 {
            self.capital_paid[actor] = self.senior[actor];
            if self.faces[actor] != 0 {
                self.created[actor] = true;
            }
            self.junior_paid[actor] = self.gross(actor);
        }
        self.check(world);
        census(world, source, false);
    }
}

fn cleanup(
    world: &mut AttributionWorld,
    source: Pubkey,
    fresh_realization: bool,
    e: &mut Evidence,
) {
    let before = world.env.market_state().1;
    let expected_recredit =
        INSURED.min(before.source_credit[3].provider_receivable_num / BOUND_SCALE);
    assert_eq!(
        expected_recredit,
        if fresh_realization { INSURED } else { 0 }
    );
    let terminal_paid = paid(world);
    for actor in 0..5 {
        let owner = world.actors[actor].owner.insecure_clone();
        let ix = deletion(world, actor, true);
        let market_rent = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        let rent = world
            .env
            .svm
            .get_account(&world.actors[actor].portfolio)
            .unwrap()
            .lamports;
        e.peak = e.peak.max(land(
            world,
            &[ix],
            &[&owner],
            &[world.env.market, world.actors[actor].portfolio],
            None,
        ));
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market_rent + rent
        );
    }
    world.env.svm.warp_to_slot(BACKING_EXPIRY + 1);
    let admin = world.env.admin.insecure_clone();
    let close = Instruction {
        program_id: world.env.program_id,
        data: ProgInstruction::CloseSlab {
            authority_epoch: world.env.control_sequences(0).authority_epoch,
        }
        .encode(),
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new(source, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(world.env.mint, false),
        ],
    };
    let mut recredited = 0;
    let mut withdrawn = 0;
    let residue = before.vault;
    for _ in 0..16 {
        let g = world.env.market_state().1;
        if g.insurance > 0 {
            assert_eq!(g.insurance, expected_recredit);
            let ix = Instruction {
                program_id: world.env.program_id,
                data: ProgInstruction::WithdrawInsuranceAsset {
                    asset_index: 1,
                    market_id: world.env.asset_market_id(1),
                    authority_epoch: world.env.control_sequences(1).authority_epoch,
                    amount: g.insurance,
                }
                .encode(),
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            };
            e.peak = e.peak.max(land(
                world,
                &[ix],
                &[&admin],
                &[world.env.market, world.env.vault, source],
                None,
            ));
            withdrawn += g.insurance;
            assert_eq!(
                withdrawn, expected_recredit,
                "insurance atoms cannot be withdrawn twice"
            );
            census(world, source, true);
            continue;
        }
        let frame = world.frame();
        let supply = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
            .unwrap()
            .supply;
        e.peak = e.peak.max(land(
            world,
            &[close.clone()],
            &[&admin],
            &[
                world.env.market,
                world.env.vault,
                world.env.mint,
                admin.pubkey(),
                source,
            ],
            None,
        ));
        let market = world.env.svm.get_account(&world.env.market).unwrap();
        assert_eq!(paid(world), terminal_paid);
        assert_ne!(world.frame(), frame, "every terminal scan must progress");
        if market.data.len() == percolator_prog::constants::HEADER_LEN {
            let burned = supply
                - Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                    .unwrap()
                    .supply;
            assert_eq!(withdrawn + burned as u128, residue);
            assert_eq!(world.env.token_amount(source) as u128, withdrawn);
            assert_eq!(recredited, expected_recredit);
            e.recredit += usize::from(recredited != 0);
            return;
        }
        let next = world.env.market_state().1;
        recredited += next.insurance - g.insurance;
        assert_eq!(next.insurance_domain_spent[2], INSURED - recredited);
        assert!(recredited <= expected_recredit);
        census(world, source, true);
    }
    panic!("funded terminal state has no bounded public cleanup path");
}

#[test]
fn v16_program_mixed_adl_underfunded_receipt_preserves_senior_exit_and_expiry_progress() {
    let mut e = Evidence::default();
    let mut worlds = 0;
    for lots in [1, 2] {
        let mut expected = None;
        for slot in [BACKING_EXPIRY - 1, BACKING_EXPIRY, BACKING_EXPIRY + 1] {
            for peer_first in [false, true] {
                for claim in [false, true] {
                    let (mut world, source) = setup(lots, &mut e);
                    let mut book = ReceiptBook::new(&world, lots);
                    let entitlement: [u128; 5] =
                        std::array::from_fn(|i| book.senior[i] + book.faces[i]);
                    if let Some(expected) = expected {
                        assert_eq!(entitlement, expected);
                    } else {
                        expected = Some(entitlement);
                        println!("mixed debt={} atoms: checkpoint entitlements={entitlement:?}, initial receipt face={}, paid={}",
                            50 * lots, book.faces[0], book.junior_paid[0]);
                    }
                    let seed_frame = world.frame();
                    book.step(&mut world, 0, true, source, &mut e);
                    assert_eq!(
                        world.frame(),
                        seed_frame,
                        "unfunded retry cannot consume another owner's capital"
                    );
                    e.retries += 1;
                    world.env.svm.warp_to_slot(slot);
                    let order = if peer_first { [2, 4, 0] } else { [0, 4, 2] };
                    for round in 0..16 {
                        if (0..5).all(|i| {
                            resolved_portfolio_is_terminal(&world.env, world.actors[i].portfolio)
                        }) {
                            break;
                        }
                        let before = world.frame();
                        for actor in order {
                            if resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                continue;
                            }
                            let has_receipt = resolved_receipt(
                                &world.env.portfolio_state(world.actors[actor].portfolio),
                            )
                            .present;
                            book.step(&mut world, actor, claim && has_receipt, source, &mut e);
                        }
                        assert_ne!(world.frame(), before, "funded nonterminal fixed point: lots={lots}, slot={slot}, peer_first={peer_first}, claim={claim}, round={round}");
                    }
                    assert!((0..5).all(|i| resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[i].portfolio
                    )));
                    assert_eq!(paid(&world), entitlement);
                    for actor in [0, 2] {
                        let before = world.frame();
                        book.step(&mut world, actor, true, source, &mut e);
                        assert_eq!(world.frame(), before);
                        e.retries += 1;
                    }
                    cleanup(&mut world, source, slot < BACKING_EXPIRY, &mut e);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 24);
    assert_eq!(e.recredit, 8);
    assert!(e.waits > 0);
    assert_cu_within("mixed underfunded receipt campaign", e.peak, 600_000);
    println!("row419/435 mixed underfunded receipts: {worlds} worlds, {} rollbacks, {} waiting rollbacks, {} retries, {} insurance recredits, peak CU={}", e.rollbacks, e.waits, e.retries, e.recredit, e.peak);
}
