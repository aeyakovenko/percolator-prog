//! Row 411: retained fee consent after two public ADL reductions. Different
//! nonunit bases must resize independently while every route charges effective
//! fill, including policy detours, successful-prefix rollback and owner exits.
//! Bounded INV-010/011/014/024/036/047/081 evidence; no position-image injection.

use super::*;
use crate::support::reference_math::{mul_div_ceil, mul_div_floor};

const CAP: u64 = 37;
const RESIDUAL: u128 = POS_SCALE + 3;

fn charge(quantity: u128, bps: u64) -> u128 {
    let notional = mul_div_ceil(quantity, PRICE.into(), POS_SCALE).unwrap();
    mul_div_ceil(notional, bps.into(), 10_000).unwrap()
}

fn reduction(w: &World, cpi: bool, batch: bool, size: i128) -> Instruction {
    let [a, b] = w.portfolios;
    let request = match (cpi, batch) {
        (false, false) => w.env.trade_no_cpi_ix(a, b, 0, size, PRICE, CAP),
        (true, false) => w.env.trade_cpi_ix(a, b, 0, size, CAP, PRICE),
        (false, true) => w.env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: w.env.asset_market_id(0),
                size_q: size,
                exec_price: PRICE,
                fee_bps: CAP,
            }],
        ),
        (true, true) => w.env.batch_trade_cpi_ix_with_caps(
            a,
            b,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: w.env.asset_market_id(0),
                size_q: size,
                limit_price: PRICE,
                fee_bps: LP_CAP_BPS.into(),
            }],
            0,
            charge(size.unsigned_abs(), CAP),
        ),
    };
    let accounts = if cpi {
        vec![
            AccountMeta::new(w.owners[0].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
            AccountMeta::new_readonly(w.matcher, false),
            AccountMeta::new(w.context, false),
            AccountMeta::new_readonly(w.delegate, false),
        ]
    } else {
        vec![
            AccountMeta::new(w.owners[0].pubkey(), true),
            AccountMeta::new(w.owners[1].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ]
    };
    Instruction {
        program_id: w.env.program_id,
        accounts,
        data: request.encode(),
    }
}

fn check(
    w: &World,
    direction: i128,
    indices: [u128; 2],
    raw: [u128; 2],
    remaining: u128,
    fees: u128,
    deposited: bool,
) {
    let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
    let (_, group) = w.env.market_state();
    for actor in 0..2 {
        let p = &accounts[actor];
        assert_eq!(p.owner, w.owners[actor].pubkey().to_bytes());
        let deposit = if actor == 0 && deposited { PREFIX } else { 0 };
        assert_eq!(
            p.capital.get(),
            u128::from(DEPOSITS[actor] + deposit) - fees
        );
        assert_eq!(
            (p.pnl.get(), p.reserved_pnl.get(), p.fee_credits.get()),
            (0, 0, 0)
        );
        if remaining == 0 {
            assert!(!has_active_leg_for_asset(p, 0));
        } else {
            let leg = active_leg_for_asset(p, 0);
            let sign = if actor == 0 { direction } else { -direction };
            assert_eq!(leg.basis_pos_q, sign * raw[actor] as i128);
            assert_eq!(leg.a_basis, ADL_ONE);
            assert_eq!(
                mul_div_ceil(raw[actor], indices[actor], ADL_ONE).unwrap(),
                remaining
            );
        }
    }
    if remaining != 0 {
        assert_eq!(
            [group.assets[0].a_long, group.assets[0].a_short],
            if direction > 0 {
                indices
            } else {
                [indices[1], indices[0]]
            }
        );
    }
    assert_eq!(group.assets[0].effective_price, PRICE);
    assert_eq!(
        [
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q
        ],
        [remaining; 2]
    );
    assert_eq!(&group.insurance_domain_budget[..2], &[fees; 2]);
    assert!(group.insurance_domain_budget[2..]
        .iter()
        .all(|amount| *amount == 0));
    assert_eq!(group.insurance, 2 * fees);
    let vault = DEPOSITS.iter().sum::<u64>() + if deposited { PREFIX } else { 0 };
    assert_eq!(group.c_tot, u128::from(vault) - 2 * fees);
    assert_eq!(group.vault, u128::from(vault));
    assert_eq!(w.env.token_amount(w.env.vault), vault);
    assert_eq!(
        w.env.token_amount(w.sources[0]),
        if deposited { 0 } else { PREFIX }
    );
    assert_eq!(w.env.token_amount(w.sources[1]), 0);
    let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, DEPOSITS.iter().sum::<u64>() + PREFIX);
    assert_eq!(mint.mint_authority, COption::None);
    assert_market_stock_census(
        "retained dual ADL fees",
        &group,
        &w.env.svm.get_account(&w.env.market).unwrap().data,
        &accounts,
        vault.into(),
    )
    .unwrap();
    assert_reservation_encumbrance_census("retained dual ADL fees", &group, &accounts).unwrap();
}

#[test]
fn v16_retained_dual_adl_reductions_bound_effective_fees_across_policy_and_routes() {
    let matcher = std::fs::read(auth_matcher_program_path()).unwrap();
    let open = QUANTITY.unsigned_abs();
    let first = open - open / 3;
    let effective = first - first / 4;
    let indices = [
        mul_div_floor(ADL_ONE, effective, first).unwrap(),
        mul_div_floor(ADL_ONE, first, open).unwrap(),
    ];
    let raw = [
        first,
        mul_div_floor(effective, ADL_ONE, indices[1]).unwrap(),
    ];
    let resized = indices.map(|a| mul_div_floor(RESIDUAL, ADL_ONE, a).unwrap());
    let quantity = effective - RESIDUAL;
    assert_ne!(indices[0], indices[1]);
    assert!(indices.iter().all(|a| *a > 0 && *a < ADL_ONE));
    // A raw-basis fee oracle would overcharge both roles, by different amounts.
    for actor in 0..2 {
        assert!(charge(raw[actor] - resized[actor], CAP) > charge(quantity, CAP));
        assert_ne!(raw[actor] - quantity, resized[actor]);
    }
    let (mut worlds, mut rollbacks, mut peak_cu) = (0, 0, 0);
    for direction in [-1, 1] {
        for cpi in [false, true] {
            for batch in [false, true] {
                for final_rate in [7, CAP] {
                    eprintln!("dual ADL consent: direction={direction} cpi={cpi} batch={batch} rate={final_rate}");
                    let mut w = World::with_params(
                        &matcher,
                        V16CuMarketParams {
                            trade_fee_base_bps: 0,
                            ..V16CuMarketParams::default()
                        },
                    );
                    let opening = w.env.trade_no_cpi_ix(
                        w.portfolios[0],
                        w.portfolios[1],
                        0,
                        direction * open as i128,
                        PRICE,
                        0,
                    );
                    let tx = w.sign(&w.bundle_instructions(&opening)[1..], 1);
                    peak_cu = peak_cu.max(w.deliver(
                        tx,
                        false,
                        &[w.env.market, w.portfolios[0], w.portfolios[1]],
                        [1, 0, 0],
                    ));
                    w.env
                        .rebalance_reduce_with_cu(&w.owners[0], w.portfolios[0], 0, open / 3);
                    w.env
                        .rebalance_reduce_with_cu(&w.owners[1], w.portfolios[1], 0, first / 4);
                    w.env.set_matcher_config_with_trade_fee_cap(
                        w.matcher,
                        &w.owners[1],
                        w.portfolios[1],
                        w.context,
                        w.delegate,
                        1,
                        LP_CAP_BPS,
                    );
                    check(&w, direction, indices, raw, effective, 0, false);
                    peak_cu = peak_cu.max(w.policy(OLD_BPS, 2));
                    let close = reduction(&w, cpi, batch, -direction * quantity as i128);
                    let dummy = w.env.trade_cpi_ix(
                        w.portfolios[0],
                        w.portfolios[1],
                        0,
                        -direction * quantity as i128,
                        CAP,
                        PRICE,
                    );
                    let deposit = w.bundle_instructions(&dummy)[0].clone();
                    let controls = w.env.control_sequences(0);
                    let stale_policy = Instruction {
                        program_id: w.env.program_id,
                        accounts: vec![
                            AccountMeta::new(w.env.admin.pubkey(), true),
                            AccountMeta::new(w.env.market, false),
                        ],
                        data: ProgInstruction::UpdateTradeFeePolicy {
                            trade_fee_base_bps: OLD_BPS,
                            policy_sequence: controls.trade_fee + 1,
                            authority_epoch: controls.authority_epoch,
                        }
                        .encode(),
                    };
                    let retained = [
                        w.sign(&[deposit.clone(), close.clone()], 10),
                        w.sign(&[deposit.clone(), close.clone(), stale_policy], 11),
                        w.sign(&[deposit, close], 12),
                        w.sign(
                            &[reduction(&w, !cpi, !batch, -direction * quantity as i128)],
                            13,
                        ),
                    ];
                    let bytes = retained
                        .each_ref()
                        .map(|tx| bincode::serialize(tx).unwrap());
                    for tx in &retained {
                        peak_cu = peak_cu.max(w.simulate(tx));
                    }
                    let epochs = w.portfolios.map(|key| w.env.portfolio_position_epoch(key));

                    peak_cu = peak_cu.max(w.policy(CAP + 1, 20));
                    peak_cu = peak_cu.max(w.deliver(
                        retained[0].clone(),
                        true,
                        &[],
                        [1, 1, usize::from(cpi && batch)],
                    ));
                    rollbacks += 1;
                    check(&w, direction, indices, raw, effective, 0, false);
                    peak_cu = peak_cu.max(w.policy(5, 21));
                    peak_cu = peak_cu.max(w.policy(final_rate, 22));
                    let controls = w.env.control_sequences(0);
                    peak_cu = peak_cu.max(w.deliver_with_error(
                        retained[1].clone(),
                        Some((
                            4,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )),
                        &[],
                        [2, 1, usize::from(cpi)],
                    ));
                    rollbacks += 1;
                    check(&w, direction, indices, raw, effective, 0, false);
                    assert_eq!(w.env.control_sequences(0), controls);
                    let mut changed = vec![
                        w.env.market,
                        w.portfolios[0],
                        w.portfolios[1],
                        w.sources[0],
                        w.env.vault,
                    ];
                    // Batch CPI restores matcher scratch bytes after collecting its results.
                    if cpi && !batch {
                        changed.push(w.context);
                    }
                    for (tx, encoded) in retained.iter().zip(bytes) {
                        tx.verify().unwrap();
                        assert_eq!(bincode::serialize(tx).unwrap(), encoded);
                    }
                    peak_cu = peak_cu.max(w.deliver(
                        retained[2].clone(),
                        false,
                        &changed,
                        [2, 1, usize::from(cpi)],
                    ));
                    let rate = if cpi { final_rate } else { CAP };
                    let mut fees = charge(quantity, rate);
                    assert!(fees <= charge(quantity, CAP));
                    check(&w, direction, indices, resized, RESIDUAL, fees, true);
                    assert_eq!(
                        w.portfolios.map(|key| w.env.portfolio_position_epoch(key)),
                        epochs.map(|epoch| epoch + 1)
                    );
                    peak_cu = peak_cu.max(w.deliver_with_error(
                        retained[3].clone(),
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineStale as u32),
                        )),
                        &[],
                        [0, 0, 0],
                    ));
                    rollbacks += 1;
                    check(&w, direction, indices, resized, RESIDUAL, fees, true);

                    let tx = w.sign(
                        &[reduction(&w, cpi, batch, -direction * RESIDUAL as i128)],
                        30,
                    );
                    changed.retain(|key| *key != w.sources[0] && *key != w.env.vault);
                    peak_cu = peak_cu.max(w.deliver(tx, false, &changed, [1, 0, usize::from(cpi)]));
                    fees += charge(RESIDUAL, rate);
                    assert!(fees >= charge(effective, rate) && fees <= charge(effective, rate) + 2);
                    check(&w, direction, indices, [0; 2], 0, fees, true);
                    for side in [0, 1] {
                        w.env.finalize_reset_side_with_cu(0, side);
                    }
                    for actor in 0..2 {
                        let amount =
                            u128::from(DEPOSITS[actor] + if actor == 0 { PREFIX } else { 0 })
                                - fees;
                        let tx = w.sign(
                            &[Instruction {
                                program_id: w.env.program_id,
                                accounts: vec![
                                    AccountMeta::new(w.owners[actor].pubkey(), true),
                                    AccountMeta::new(w.env.market, false),
                                    AccountMeta::new(w.portfolios[actor], false),
                                    AccountMeta::new(w.sources[actor], false),
                                    AccountMeta::new(w.env.vault, false),
                                    AccountMeta::new_readonly(w.env.vault_authority, false),
                                    AccountMeta::new_readonly(spl_token::ID, false),
                                ],
                                data: w.env.withdraw_ix(w.portfolios[actor], amount).encode(),
                            }],
                            40 + actor as u32,
                        );
                        peak_cu = peak_cu.max(w.deliver(
                            tx,
                            false,
                            &[
                                w.env.market,
                                w.portfolios[actor],
                                w.sources[actor],
                                w.env.vault,
                            ],
                            [1, 1, 0],
                        ));
                        assert_eq!(w.env.token_amount(w.sources[actor]) as u128, amount);
                        assert_eq!(w.env.portfolio_state(w.portfolios[actor]).capital.get(), 0);
                    }
                    let (_, group) = w.env.market_state();
                    assert_eq!(
                        (group.c_tot, group.insurance, group.vault),
                        (0, 2 * fees, 2 * fees)
                    );
                    assert_eq!(w.env.token_amount(w.env.vault) as u128, 2 * fees);
                    assert_eq!(
                        w.sources
                            .map(|key| w.env.token_amount(key))
                            .iter()
                            .sum::<u64>()
                            + w.env.token_amount(w.env.vault),
                        DEPOSITS.iter().sum::<u64>() + PREFIX
                    );
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, rollbacks), (16, 48));
    eprintln!("retained dual ADL fees: {worlds} worlds, {rollbacks} rollbacks, 32 fills, 32 payouts; peak {peak_cu} CU");
}
