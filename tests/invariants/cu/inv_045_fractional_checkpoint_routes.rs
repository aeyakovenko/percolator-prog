//! Scope R: fractional owner floors across retained funding-checkpoint routes.
//! Two activation boundaries distinguish the old funding debt from the new mark.

use super::*;
use funding_carry_entitlement::retained_funding_retry::{reject, sign};

const RATE: i128 = 10_000;
const LIMIT: u64 = 600_000;

fn retained_reduction(
    world: &World,
    batch: bool,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    price: u64,
) -> Instruction {
    let env = &world.env;
    let [a, b, _, _] = world.portfolios;
    let q = -3 * SCALE / 8;
    let mut accounts = vec![AccountMeta::new(world.owners[0].pubkey(), true)];
    if matcher.is_none() {
        accounts.push(AccountMeta::new(world.owners[1].pubkey(), true));
    }
    accounts.extend([
        AccountMeta::new(env.market, false),
        AccountMeta::new(a, false),
        AccountMeta::new(b, false),
    ]);
    if let Some((program, context, delegate)) = matcher {
        accounts.extend([
            AccountMeta::new_readonly(program, false),
            AccountMeta::new(context, false),
            AccountMeta::new_readonly(delegate, false),
        ]);
    }
    let data = match (batch, matcher.is_some()) {
        (false, false) => env.trade_no_cpi_ix(a, b, 0, q, price, 0),
        (false, true) => env.trade_cpi_ix(a, b, 0, q, 0, price),
        (true, false) => env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q: q,
                exec_price: price,
                fee_bps: 0,
            }],
        ),
        (true, true) => env.batch_trade_cpi_ix(
            a,
            b,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q: q,
                limit_price: price,
                fee_bps: 0,
            }],
        ),
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: data.encode(),
    }
}

fn advance_one(world: &mut World, ledger: &mut Ledger, reverse: bool, rollback: bool) {
    let clock = world.env.svm.get_sysvar::<Clock>().slot;
    let next = ledger.slot + 1;
    assert!(next <= clock);
    world.trace.push(format!("clock={clock}, accrue={next}"));
    let ix = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.payer.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.portfolios[2], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: clock,
            observations: crank_observations_for_assets(&if reverse { [1, 0] } else { [0, 1] }),
        }
        .encode(),
    };
    if rollback {
        let tx = sign(
            world,
            &[
                ix.clone(),
                Instruction {
                    program_id: world.env.program_id,
                    accounts: vec![],
                    data: vec![],
                },
            ],
            200 + next as u32,
        );
        reject(world, &tx, 3, InstructionError::InvalidInstructionData);
        ledger.check(world);
    }
    let absent = [0, 1, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
    let tx = sign(world, &[ix], 100 + next as u32);
    let meta = world
        .env
        .svm
        .send_transaction(tx)
        .expect("bounded checkpoint progress");
    world.max_cu = world.max_cu.max(meta.compute_units_consumed);
    ledger.advance(next);
    // A bounded partial market step does not settle the supplied portfolio.
    if next == clock {
        ledger.settle(2);
    }
    ledger.check(world);
    assert_eq!(
        [0, 1, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
        absent
    );
}

#[test]
fn v16_program_fractional_checkpoint_retries_preserve_four_route_owner_residues() {
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    let mut separate_floors = 0;
    let mut residues = std::collections::BTreeSet::new();
    for direction in [-1, 1] {
        for boundary in [5, 7] {
            let mut reference = None;
            for cpi in [false, true] {
                for batch in [false, true] {
                    for interrupted in [false, true] {
                        let history = History {
                            direction,
                            batch,
                            split: false,
                            placement: 0,
                            reverse: batch,
                        };
                        let mut world = World::with_funding(history, RATE as u64);
                        world.trace.push(format!(
                            "Scope R: direction={direction}, boundary={boundary}, cpi={cpi}, batch={batch}, interrupted={interrupted}"
                        ));
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
                        let mut ledger = Ledger::new(direction, RATE);
                        ledger.check(&world);
                        reduce(
                            &mut world,
                            &mut ledger,
                            0,
                            [SCALE / 4, SCALE / 2],
                            true,
                            false,
                            None,
                            false,
                        );
                        for slot in 1..=2 {
                            world.env.svm.warp_to_slot(slot);
                            advance_one(&mut world, &mut ledger, batch, false);
                        }
                        assert_eq!(ledger.carry(), [4_800, 6_000]);
                        let matcher = cpi.then(|| {
                            let matcher = auth_matcher_for_lp_via_system_create(
                                &mut world.env,
                                &world.owners[1],
                                world.portfolios[1],
                            );
                            world.env.set_matcher_config_with_trade_fee_cap(
                                matcher.0,
                                &world.owners[1],
                                world.portfolios[1],
                                matcher.1,
                                matcher.2,
                                1,
                                0,
                            );
                            matcher
                        });
                        ledger.check(&world);
                        let expected_price = (i128::from(ANCHORS[0])
                            - direction
                                * i128::from(ANCHORS[0] * CAP_BPS * (boundary - 2) / 10_000))
                            as u64;
                        let ix = retained_reduction(&world, batch, matcher, expected_price);
                        let retained =
                            [0, 1, 2].map(|nonce| sign(&world, std::slice::from_ref(&ix), nonce));
                        let bytes = retained
                            .each_ref()
                            .map(|tx| bincode::serialize(tx).unwrap());
                        let late = sign(
                            &world,
                            &[
                                ix,
                                Instruction {
                                    program_id: world.env.program_id,
                                    accounts: vec![],
                                    data: vec![],
                                },
                            ],
                            3,
                        );
                        let late_bytes = bincode::serialize(&late).unwrap();
                        let owners = [0, 1, 3]
                            .map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
                        world.env.svm.warp_to_slot(boundary);
                        for asset in if batch { [1, 0] } else { [0, 1] } {
                            let target = i128::from(ANCHORS[asset])
                                - direction * if asset == 0 { 20 } else { -20 };
                            let cu = world.env.push_auth_mark_for_asset_as_admin(
                                asset as u16,
                                boundary,
                                target as u64,
                            );
                            world.max_cu = world.max_cu.max(cu);
                            ledger.publish_at(asset, target, boundary);
                            ledger.check(&world);
                        }
                        assert_eq!(ledger.carry(), [0; 2]);
                        assert!(ledger.pending_funding.iter().all(Option::is_some));
                        for attempt in 0..2 {
                            if interrupted {
                                reject(
                                    &mut world,
                                    &retained[attempt],
                                    2,
                                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                                );
                                ledger.check(&world);
                                rollbacks += 1;
                            }
                            if attempt == 0 {
                                advance_one(&mut world, &mut ledger, batch, false);
                            }
                        }
                        while ledger.slot < boundary {
                            let activation = ledger.slot + 1 == boundary;
                            advance_one(&mut world, &mut ledger, batch, interrupted && activation);
                            rollbacks += usize::from(interrupted && activation);
                        }
                        assert_eq!(ledger.price[0], expected_price);
                        assert!(ledger.carry().iter().all(|carry| *carry > 0));
                        assert_eq!(ledger.pending_funding, [None; 2]);
                        assert_eq!(
                            [0, 1, 3]
                                .map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
                            owners
                        );
                        assert_eq!(
                            retained
                                .each_ref()
                                .map(|tx| bincode::serialize(tx).unwrap()),
                            bytes
                        );
                        assert_eq!(bincode::serialize(&late).unwrap(), late_bytes);
                        if interrupted {
                            reject(
                                &mut world,
                                &late,
                                3,
                                InstructionError::InvalidInstructionData,
                            );
                            ledger.check(&world);
                            rollbacks += 1;
                        }
                        let profiles = carry_transport_exit::profiles(&world);
                        let meta = world
                            .env
                            .svm
                            .send_transaction(retained[2].clone())
                            .expect("retained fractional reduction");
                        world.max_cu = world.max_cu.max(meta.compute_units_consumed);
                        ledger.settle(0);
                        ledger.settle(1);
                        ledger.q[0][0] -= 3 * SCALE / 8;
                        ledger.q[1][0] += 3 * SCALE / 8;
                        ledger.check(&world);
                        assert_eq!(carry_transport_exit::profiles(&world), profiles);
                        for slot in boundary + 1..=boundary + 4 {
                            world.env.svm.warp_to_slot(slot);
                            advance_one(&mut world, &mut ledger, batch, false);
                        }
                        assert_eq!(world.env.svm.get_account(&world.portfolios[3]), owners[2]);
                        for actor in [0, 2] {
                            let q = ledger.q[actor];
                            reduce(&mut world, &mut ledger, actor, q, true, batch, None, false);
                        }
                        let residue_num = ledger.residue_num.iter().flatten().sum::<i128>();
                        assert_eq!(residue_num % SCALE, 0);
                        let residue = u128::try_from(residue_num / SCALE).unwrap();
                        assert!(residue > 0);
                        assert!(ledger.residue_num.iter().any(|r| r[0] > 0 && r[1] > 0));
                        assert!(ledger.latent_checks > 0);
                        separate_floors += ledger.separate_floor_checks;
                        residues.insert(residue);
                        let endpoint = Economics {
                            price: ledger.price,
                            carry: ledger.carry(),
                            lots: [[0; 2]; 4],
                            entitlement: ledger.value,
                            vault: PRINCIPAL.map(u128::from).iter().sum(),
                        };
                        let paid = carry_transport_exit::pay_resolved_with_residue(
                            &mut world,
                            &endpoint,
                            batch,
                            ledger.slot + 101,
                            residue,
                            |_, _, _| {},
                        );
                        assert_eq!(paid.map(i128::from), ledger.value);
                        let group = world.env.market_state().1;
                        assert_eq!(
                            (
                                group.vault,
                                group.insurance,
                                group.backing_provider_earnings_total
                            ),
                            (residue, 0, 0)
                        );
                        // Untouched expired buckets can retain a raw Fresh tag.
                        // They are not current backing at the input payout slot.
                        for bucket in &group.source_backing_buckets {
                            if bucket.status == percolator::BackingBucketStatusV16::Fresh
                                && bucket.fresh_unliened_backing_num > 0
                            {
                                assert!(bucket.expiry_slot <= ledger.slot + 101);
                            }
                            assert_eq!(
                                (
                                    bucket.valid_liened_backing_num,
                                    bucket.impaired_liened_backing_num,
                                    bucket.utilization_fee_earnings
                                ),
                                (0, 0, 0)
                            );
                        }
                        let outcome = (
                            endpoint,
                            ledger.funding,
                            ledger.ideal_num,
                            ledger.residue_num,
                            ledger.funding_flows,
                            paid,
                        );
                        if let Some(expected) = &reference {
                            assert_eq!(&outcome, expected);
                        } else {
                            reference = Some(outcome);
                        }
                        peak = peak.max(world.max_cu);
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!((worlds, rollbacks), (32, 64));
    assert!(separate_floors > 0);
    assert!(residues.len() > 1);
    assert_cu_within("fractional checkpoint route retry", peak, LIMIT);
    eprintln!("Scope R: worlds={worlds}, exact_rollbacks={rollbacks}, payouts=128, residues={residues:?}, separate_floors={separate_floors}, peak_cu={peak}");
}
