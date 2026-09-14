//! INV-058: capped disjoint pairs retain earned PnL when existing legs exchange
//! headroom, then resolve with exposure still open. Current OI is not a claim key.
//! Public construction; integral AuthMark PnL, unit ADL, no fees or provider stock.
//! This finite composition does not close rows 417/423/424/427 or generic capacity.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};

#[derive(Default)]
struct Measurements {
    calls: usize,
    rollbacks: usize,
    terminal: usize,
    transfers: usize,
    peak: u64,
}

fn land(
    w: &mut World,
    m: &mut Measurements,
    instructions: &[Instruction],
    changed: &[Pubkey],
    abort: bool,
) {
    let mut ixs = instructions.to_vec();
    if abort {
        // Insufficient System funds abort only after the public prefix.
        ixs.push(system_instruction::transfer(
            &w.owners[0].pubkey(),
            &w.owners[1].pubkey(),
            u64::MAX,
        ));
    }
    let tx = w.transaction(&ixs);
    assert!(bincode::serialize(&tx).unwrap().len() <= 1232);
    if !abort
        && instructions
            .iter()
            .all(|ix| ix.accounts.iter().all(|a| !a.is_signer))
    {
        assert_eq!(
            tx.signatures.len(),
            1,
            "permissionless economic disposition"
        );
    }
    let before = frame(&w.env, &tx, &w.keys());
    let fee = FeeStructure::default().lamports_per_signature * tx.signatures.len() as u64;
    let result = w.env.svm.send_transaction(tx);
    let meta = if abort {
        let error = result.expect_err("successful prefix must be atomic with the System suffix");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(
                instructions.len() as u8 + 2,
                InstructionError::Custom(1)
            )
        );
        check_frame(&w.env, before, fee, &[]);
        m.rollbacks += 1;
        error.meta
    } else {
        let meta = result.unwrap_or_else(|e| panic!("PnL handoff continuation: {e:?}"));
        check_frame(&w.env, before, fee, changed);
        meta
    };
    assert_eq!(
        meta.logs
            .iter()
            .filter(|line| **line == format!("Program {} success", w.env.program_id))
            .count(),
        instructions.len(),
        "every tested prefix must execute"
    );
    m.transfers += meta
        .logs
        .iter()
        .filter(|line| line.contains("Instruction: Transfer"))
        .count();
    assert_cu_within(
        "capped PnL handoff/payout",
        meta.compute_units_consumed,
        900_000,
    );
    m.peak = m.peak.max(meta.compute_units_consumed);
    m.calls += 1;
}

fn trade(w: &World, pair: usize, q: i128, price: u64, batch: bool) -> Instruction {
    let a = w.portfolios[pair];
    let b = w.portfolios[pair + 1];
    let ix = if batch {
        w.env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: w.env.asset_market_id(0),
                size_q: q,
                exec_price: price,
                fee_bps: 0,
            }],
        )
    } else {
        w.env.trade_no_cpi_ix(a, b, 0, q, price, 0)
    };
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(w.owners[pair].pubkey(), true),
            AccountMeta::new(w.owners[pair + 1].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ],
        data: ix.encode(),
    }
}

pub(super) fn payout(w: &World, actor: usize) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(w.owners[actor].pubkey(), false),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.portfolios[actor], false),
            AccountMeta::new(w.tokens[actor], false),
            AccountMeta::new(w.env.vault, false),
            AccountMeta::new_readonly(w.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    }
}

fn check(w: &World, gain: [i128; ACTORS], ids: [u64; ACTORS], price: u64) -> [u128; ACTORS] {
    let group = w.env.market_state().1;
    let accounts = w.portfolios.map(|p| w.env.portfolio_state(p));
    let mut oi = [0; 2];
    let mut counts = [0; 2];
    let mut bound = 0;
    let mut paid = 0;
    let mut claims = [0; ACTORS];
    for actor in 0..ACTORS {
        let p = &accounts[actor];
        let data = w.env.svm.get_account(&w.portfolios[actor]).unwrap().data;
        let (provenance, owner) = state::read_portfolio_owner_preflight(&data).unwrap();
        assert_eq!(provenance.market_group_id, w.env.market.to_bytes());
        assert_eq!(
            provenance.portfolio_account_id,
            w.portfolios[actor].to_bytes()
        );
        assert_eq!(owner, w.owners[actor].pubkey().to_bytes());
        assert_eq!(w.env.portfolio_id(w.portfolios[actor]), ids[actor]);
        let tokens = w.env.token_amount(w.tokens[actor]) as u128;
        assert_eq!(
            resolved_receipt(p),
            ResolvedPayoutReceiptV16::EMPTY,
            "fully backed source realization needs no outstanding haircut receipt"
        );
        assert!(p.pnl.get() >= 0);
        claims[actor] = p.capital.get() + p.pnl.get() as u128 + tokens;
        assert_eq!(p.reserved_pnl.get(), 0);
        assert_eq!(p.fee_credits.get(), 0);
        paid += tokens;
        for source in p.source_domains.iter().filter(|s| s.is_occupied()) {
            assert_eq!(source.domain.get(), u32::from(w.positions[0][0] > 0));
            assert_eq!(
                source.source_claim_market_id.get(),
                group.assets[0].market_id
            );
            assert_eq!(
                source.source_claim_bound_num.get(),
                gain[actor].max(0) as u128 * BOUND_SCALE
            );
            assert_eq!(source.source_claim_liened_num.get(), 0);
            bound += source.source_claim_bound_num.get();
        }
        for encoded in &p.legs {
            let leg = encoded.try_to_runtime().unwrap();
            if !leg.active {
                continue;
            }
            assert_eq!(leg.asset_index, 0);
            assert_eq!(leg.market_id, group.assets[0].market_id);
            assert_eq!(leg.basis_pos_q, w.positions[actor][0]);
            assert_eq!(leg.a_basis, ADL_ONE);
            assert!(leg.basis_pos_q.unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
            let side = usize::from(leg.basis_pos_q < 0);
            oi[side] += leg.basis_pos_q.unsigned_abs();
            counts[side] += 1;
        }
        if group.mode == MarketModeV16::Live {
            assert_eq!(percolator::active_bitmap_count_ones(active_bitmap(p)), 1);
            assert_eq!(
                w.env.portfolio_position_epoch(w.portfolios[actor]),
                w.epochs[actor]
            );
        }
    }
    assert_eq!(
        claims,
        gain.map(|g| (CAPITAL as i128 + g) as u128),
        "input-owned claims survive exposure handoff and terminal consumption"
    );
    let a = group.assets[0];
    assert_eq!([a.effective_price, a.raw_oracle_target_price], [price; 2]);
    assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
    assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi);
    assert_eq!([a.stored_pos_count_long, a.stored_pos_count_short], counts);
    assert!(oi.into_iter().all(|q| q <= percolator::MAX_OI_SIDE_Q));
    assert_eq!(group.source_claim_bound_total_num, bound);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault + paid, SUPPLY);
    let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
    assert_eq!(
        (mint.supply as u128, mint.mint_authority),
        (SUPPLY, COption::None)
    );
    let market = w.env.svm.get_account(&w.env.market).unwrap();
    assert_market_stock_census(
        "capped PnL handoff",
        &group,
        &market.data,
        &accounts,
        w.env.token_amount(w.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("capped PnL handoff", &group, &accounts).unwrap();
    assert_source_credit_rates("capped PnL handoff", &group).unwrap();
    claims
}

#[test]
fn v16_program_capped_pair_pnl_survives_headroom_handoff_and_ranked_terminal_payout() {
    assert_certified_engine_pin("INV-058 PnL terminal handoff");
    let cap = percolator::MAX_OI_SIDE_Q;
    let q = [cap / 4, cap / 4 + POS_SCALE, cap / 2 - POS_SCALE];
    assert_eq!(q.iter().sum::<u128>(), cap);
    assert!(q.iter().all(|n| n % POS_SCALE == 0));
    let gain =
        std::array::from_fn(|a| (q[a / 2] / POS_SCALE) as i128 * if a % 2 == 0 { 1 } else { -1 });
    let expected = gain.map(|g| (CAPITAL as i128 + g) as u128);
    let mut m = Measurements::default();
    let mut worlds = 0;
    for direction in [-1i128, 1] {
        for batch in [false, true] {
            for reverse in [false, true] {
                let mut w = World::new();
                w.env.configure_permissionless_resolve_with_cu(1000, 5);
                let ids = w.portfolios.map(|p| w.env.portfolio_id(p));
                for pair in [0, 2, 4] {
                    let size = direction * q[pair / 2] as i128;
                    let ix = trade(&w, pair, size, PRICE, batch);
                    w.accept(&[ix], &[pair]);
                    w.record(pair, &[(0, size)], 0, 1);
                    w.check();
                }
                m.peak = m.peak.max(w.peak[1]);
                assert_eq!(w.env.market_state().1.assets[0].oi_eff_long_q, cap);
                let price = (PRICE as i128 + direction) as u64;
                w.env.svm.warp_to_slot(1);
                m.peak = m
                    .peak
                    .max(w.env.push_auth_mark_for_asset_as_admin(0, 1, price));
                let order = if reverse {
                    [4, 2, 0, 5, 3, 1]
                } else {
                    [1, 3, 5, 0, 2, 4]
                };
                for actor in order {
                    for _ in 0..4 {
                        let p = w.env.portfolio_state(w.portfolios[actor]);
                        if p.pnl.get() == gain[actor].max(0)
                            && p.capital.get() == (CAPITAL as i128 + gain[actor].min(0)) as u128
                        {
                            break;
                        }
                        let cu = w.env.crank(
                            w.portfolios[actor],
                            ProgInstruction::PermissionlessCrank {
                                now_slot: 1,
                                observations: crank_observations(0),
                            },
                        );
                        m.peak = m.peak.max(cu);
                    }
                }
                check(&w, gain, ids, price);

                let release = direction * (2 * POS_SCALE) as i128;
                let first = trade(&w, 0, -release, price, batch);
                let second = trade(&w, 2, release, price, !batch);
                land(&mut w, &mut m, &[first.clone(), second.clone()], &[], true);
                check(&w, gain, ids, price);
                for (pair, size, ix) in [(0, -release, first), (2, release, second)] {
                    let changed = [w.env.market, w.portfolios[pair], w.portfolios[pair + 1]];
                    land(&mut w, &mut m, &[ix], &changed, false);
                    w.record(pair, &[(0, size)], 0, 1);
                    check(&w, gain, ids, price);
                    assert_eq!(
                        w.env.market_state().1.assets[0].oi_eff_long_q,
                        cap - if pair == 0 { release.unsigned_abs() } else { 0 }
                    );
                }
                let mut wrong_claims = check(&w, gain, ids, price);
                wrong_claims.swap(0, 2);
                assert_eq!(
                    wrong_claims.iter().sum::<u128>(),
                    expected.iter().sum::<u128>()
                );
                assert_ne!(
                    wrong_claims, expected,
                    "aggregate-neutral owner swap fails the claim vector"
                );
                assert_ne!(
                    w.positions[0][0].unsigned_abs() / POS_SCALE,
                    gain[0] as u128,
                    "current exposure cannot reconstruct the earlier claim"
                );

                let portfolios = w.portfolios.map(|p| w.env.svm.get_account(&p));
                m.peak = m.peak.max(w.env.resolve());
                assert_eq!(w.portfolios.map(|p| w.env.svm.get_account(&p)), portfolios);
                assert_eq!(w.env.market_state().1.mode, MarketModeV16::Resolved);
                w.env.svm.warp_to_slot(6);
                let rank = |w: &World, actor: usize| {
                    let p = w.env.portfolio_state(w.portfolios[actor]);
                    (
                        percolator::active_bitmap_count_ones(active_bitmap(&p)),
                        p.source_domains.iter().filter(|s| s.is_occupied()).count(),
                        expected[actor] - w.env.token_amount(w.tokens[actor]) as u128,
                    )
                };
                // Detach the cohort before asking a flat positive claimant to pay.
                // A pending peer is a prerequisite, not account-local nonprogress.
                for _ in 0..4 {
                    for actor in order {
                        if rank(&w, actor) == (0, 0, 0) {
                            continue;
                        }
                        if rank(&w, actor).0 == 0
                            && gain[actor] > 0
                            && (0..ACTORS).any(|peer| rank(&w, peer).0 != 0)
                        {
                            continue;
                        }
                        let before = rank(&w, actor);
                        let ix = payout(&w, actor);
                        let changed = [
                            w.env.market,
                            w.env.vault,
                            w.portfolios[actor],
                            w.tokens[actor],
                        ];
                        land(&mut w, &mut m, &[ix.clone()], &[], true);
                        check(&w, gain, ids, price);
                        land(&mut w, &mut m, &[ix], &changed, false);
                        assert!(
                            rank(&w, actor) < before,
                            "each accepted terminal call decreases rank"
                        );
                        m.terminal += 1;
                        check(&w, gain, ids, price);
                    }
                }
                for actor in 0..ACTORS {
                    assert_eq!(rank(&w, actor), (0, 0, 0), "four-sweep terminal bound");
                    assert!(resolved_portfolio_is_terminal(&w.env, w.portfolios[actor]));
                }
                assert_eq!(w.tokens.map(|t| w.env.token_amount(t) as u128), expected);
                for actor in 0..ACTORS {
                    w.env.svm.expire_blockhash();
                    m.peak = m.peak.max(
                        w.env
                            .close_portfolio_with_cu(&w.owners[actor], w.portfolios[actor]),
                    );
                }
                let group = w.env.market_state().1;
                assert_eq!(
                    (
                        group.vault,
                        group.c_tot,
                        group.source_claim_bound_total_num,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0, 0)
                );
                assert!(group
                    .assets
                    .iter()
                    .all(|a| a.oi_eff_long_q == 0 && a.oi_eff_short_q == 0));
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert!(
        m.transfers >= ACTORS * worlds * 2,
        "actual SPL prefixes abort and retry"
    );
    assert_cu_within("PnL handoff campaign including helpers", m.peak, 900_000);
    println!("INV-058 PnL terminal handoff: worlds={worlds}, checked_calls={}, rollbacks={}, terminal_calls={}, transfer_logs={}, peak_cu={}",
        m.calls, m.rollbacks, m.terminal, m.transfers, m.peak);
}
