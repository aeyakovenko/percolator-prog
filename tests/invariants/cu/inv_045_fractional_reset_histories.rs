//! Scope I / row 425: nonzero carry resets with fractional K/F owner frontiers.
//! The input-owned episode ledger is shared with the fixed-target controls.

use super::*;

fn publish(world: &mut World, ledger: &mut Ledger, asset: usize, target: i128) -> u64 {
    world.trace.push(format!(
        "slot={}: reset asset={asset}, target={target}",
        ledger.slot
    ));
    let owners = world.portfolios.map(|key| world.env.svm.get_account(&key));
    let other = carry_transport_exit::profiles(world)[1 - asset];
    let sequence = world.env.control_sequences(asset);
    let ix = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(world.env.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
        ],
        data: ProgInstruction::PushAuthMark {
            market_id: world.env.asset_market_id(asset as u16),
            asset_index: asset as u16,
            now_slot: ledger.slot,
            mark_e6: u64::try_from(target).unwrap(),
            observation_sequence: next_control_sequence(sequence.oracle_observation),
            authority_epoch: sequence.authority_epoch,
        }
        .encode(),
    };
    // Reuse the complete Account executor, including the admin signature fee.
    let rejected = reject_signed_suffix(world, ix.clone(), &[], true);
    ledger.check(world);
    world.env.svm.expire_blockhash();
    let cu = send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        ix,
        &[&world.env.admin],
    )
    .unwrap();
    world.max_cu = world.max_cu.max(cu);
    ledger.publish(asset, target);
    ledger.check(world);
    assert_eq!(
        world.portfolios.map(|key| world.env.svm.get_account(&key)),
        owners
    );
    assert_eq!(carry_transport_exit::profiles(world)[1 - asset], other);
    world.max_cu.max(rejected)
}

#[test]
fn v16_program_fractional_reset_histories_preserve_route_and_residue_adjusted_owner_value() {
    let mut worlds = 0;
    let mut resets = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    let mut floor_witnesses = 0;
    let mut cadence_witnesses = 0;
    let mut residues = std::collections::BTreeSet::new();
    for direction in [-1, 1] {
        let mut deferred: Option<(
            Economics,
            [i128; 4],
            [[i128; 2]; 4],
            [[u128; 4]; 4],
            [u64; 4],
        )> = None;
        for eager in [false, true] {
            let mut route_reference = None;
            for cpi in [false, true] {
                for batch in [false, true] {
                    for publish_first in [false, true] {
                        let history = History {
                            direction,
                            batch,
                            split: false,
                            placement: 0,
                            reverse: publish_first,
                        };
                        let mut world = World::with_funding_and_accrual_limit(history, 10_000, 4);
                        world.trace.push(format!(
                            "Scope I: eager={eager}, cpi={cpi}, publish_first={publish_first}"
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
                        let matcher = auth_matcher_for_lp_via_system_create(
                            &mut world.env,
                            &world.owners[1],
                            world.portfolios[1],
                        );
                        let mut ledger = Ledger::new(direction, 10_000);
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
                        let passive = world.env.svm.get_account(&world.portfolios[3]);
                        for event in 0..4 {
                            let endpoint = 5 * (event + 1);
                            while ledger.slot < endpoint {
                                let slot = if publish_first || eager {
                                    ledger.slot + 1
                                } else {
                                    (ledger.slot + 4).min(endpoint)
                                };
                                world.env.svm.warp_to_slot(slot);
                                ledger.advance(slot);
                                crank(&mut world, &mut ledger, 2, publish_first);
                                if eager && slot % 5 == 3 {
                                    for actor in [1, 0] {
                                        crank(&mut world, &mut ledger, actor, publish_first);
                                    }
                                }
                            }
                            assert!(ledger.carry().iter().all(|carry| *carry != 0));
                            assert!(ledger.price.iter().zip(ANCHORS).all(|(p, a)| *p != a));
                            for publication in [publish_first, !publish_first] {
                                if publication {
                                    if event == 3 {
                                        continue;
                                    }
                                    for asset in if publish_first { [1, 0] } else { [0, 1] } {
                                        assert_ne!(ledger.carry()[asset], 0);
                                        let sign = direction * if asset == 0 { 1 } else { -1 };
                                        let target = i128::from(ANCHORS[asset])
                                            + sign * (27 + 7 * event as i128);
                                        peak = peak.max(publish(
                                            &mut world,
                                            &mut ledger,
                                            asset,
                                            target,
                                        ));
                                        resets += 1;
                                        rollbacks += 1;
                                    }
                                } else {
                                    peak = peak.max(reduce(
                                        &mut world,
                                        &mut ledger,
                                        0,
                                        [SCALE / 8, 3 * SCALE / 8],
                                        batch,
                                        publish_first,
                                        cpi.then_some(matcher),
                                        true,
                                    ));
                                    rollbacks += 1;
                                }
                            }
                            assert_eq!(world.env.svm.get_account(&world.portfolios[3]), passive);
                        }
                        for actor in [0, 2] {
                            let amounts = ledger.q[actor];
                            reduce(
                                &mut world,
                                &mut ledger,
                                actor,
                                amounts,
                                true,
                                publish_first,
                                None,
                                false,
                            );
                        }
                        let residue_num = ledger.residue_num.iter().flatten().sum::<i128>();
                        assert_eq!(residue_num % SCALE, 0);
                        let residue = u128::try_from(residue_num / SCALE).unwrap();
                        assert!(residue > 0 && ledger.latent_checks > 0);
                        assert!(ledger
                            .residue_num
                            .iter()
                            .any(|lanes| lanes[0] > 0 && lanes[1] > 0));
                        floor_witnesses += ledger.separate_floor_checks;
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
                            publish_first,
                            121,
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
                        assert!(group.source_backing_buckets.iter().all(|b| {
                            (b.status != percolator::BackingBucketStatusV16::Fresh
                                || b.fresh_unliened_backing_num == 0)
                                && b.valid_liened_backing_num == 0
                                && b.impaired_liened_backing_num == 0
                                && b.utilization_fee_earnings == 0
                        }));
                        let result = (
                            endpoint,
                            ledger.ideal_num,
                            ledger.residue_num,
                            ledger.funding_flows,
                            paid,
                        );
                        if let Some(reference) = &route_reference {
                            assert_eq!(&result, reference);
                        } else {
                            route_reference = Some(result);
                        }
                        peak = peak.max(world.max_cu);
                        worlds += 1;
                    }
                }
            }
            let current = route_reference.unwrap();
            if let Some((old_endpoint, ideal, old_residue, _, old_paid)) = &deferred {
                assert_eq!(current.0.price, old_endpoint.price);
                assert_eq!(current.0.carry, old_endpoint.carry);
                assert_eq!(current.1, *ideal);
                for actor in 0..4 {
                    let extra_residue = current.2[actor].iter().sum::<i128>()
                        - old_residue[actor].iter().sum::<i128>();
                    let difference = i128::from(old_paid[actor]) - i128::from(current.4[actor]);
                    assert_eq!(difference * SCALE, extra_residue);
                    assert!((0..=16).contains(&difference));
                    if actor >= 2 {
                        assert_eq!(difference, 0);
                    }
                    cadence_witnesses += usize::from(difference > 0);
                }
            } else {
                deferred = Some(current);
            }
        }
    }
    assert_eq!((worlds, resets, rollbacks), (32, 192, 320));
    assert!(floor_witnesses > 0 && cadence_witnesses > 0);
    assert_cu_within("fractional moving reset histories", peak, 600_000);
    eprintln!("Scope I carry: worlds={worlds}, nonzero_resets={resets}, rollbacks={rollbacks}, payouts=128, residues={residues:?}, separate_floors={floor_witnesses}, cadence_witnesses={cadence_witnesses}, peak_cu={peak}");
}
