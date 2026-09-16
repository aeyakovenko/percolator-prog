//! INV-067 / row 417: receipt entitlement across backing expiry AND the owner-exit
//! signature boundary. The parent supplies the input-driven stock/receipt oracle;
//! Lane 17's beneficiary succession and Lane 20's deferred fees are absent here.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn configure<const DELAY: u64>(env: &mut V16CuEnv) {
    env.configure_permissionless_resolve_with_cu(100, DELAY);
}

fn route(world: &World, actor: usize, crank: bool, signed: bool) -> Instruction {
    let mut ix = world.payout(actor, false);
    ix.accounts[0].is_signer = signed;
    if crank {
        // Retained bytes: Clock, not this old slot, must govern the exit window.
        ix.data = ProgInstruction::PermissionlessCrank {
            now_slot: 12,
            observations: vec![],
        }
        .encode();
    }
    ix
}

#[derive(Default)]
struct Evidence {
    peak_bytes: usize,
    rollbacks: usize,
    paid_rollbacks: usize,
    gate_rejections: usize,
}

fn submit(
    world: &mut World,
    evidence: &mut Evidence,
    instructions: &[Instruction],
    owners: &[usize],
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    world.env.svm.expire_blockhash();
    let instructions: Vec<_> = [heap_ix(), cu_ix()]
        .into_iter()
        .chain(instructions.iter().cloned())
        .collect();
    let signers: Vec<_> = std::iter::once(&world.env.payer)
        .chain(owners.iter().map(|&a| &world.actors[a].owner))
        .collect();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    // Solana unions signer privileges across instructions. A signed normalizer
    // must not silently turn another receipt holder's unsigned close into a signed one.
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        1 + owners.len()
    );
    for (actor, entry) in world.actors.iter().enumerate() {
        let signed = tx
            .message
            .account_keys
            .iter()
            .position(|k| *k == entry.owner.pubkey())
            .is_some_and(|index| tx.message.is_signer(index));
        assert_eq!(signed, owners.contains(&actor));
    }
    let bytes = bincode::serialize(&tx).unwrap().len();
    assert!(bytes <= 1_232, "public packet: {bytes}");
    evidence.peak_bytes = evidence.peak_bytes.max(bytes);
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = world.env.svm.send_transaction(tx);
    let meta = match &result {
        Ok(meta) => meta,
        Err(failure) => &failure.meta,
    };
    world.peak_cu = world.peak_cu.max(meta.compute_units_consumed);
    assert_cu_within(
        "receipt exit-window transaction",
        meta.compute_units_consumed,
        700_000,
    );
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
    result
}

fn successes(meta: &litesvm::types::TransactionMetadata, program: Pubkey) -> usize {
    meta.logs
        .iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn reject(
    world: &mut World,
    evidence: &mut Evidence,
    instructions: &[Instruction],
    owners: &[usize],
    expected: InstructionError,
    transfers: usize,
) {
    let before = world.frame();
    let failure = submit(world, evidence, instructions, owners).expect_err("suffix rejects");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError((instructions.len() + 1) as u8, expected.clone())
    );
    assert_eq!(
        successes(&failure.meta, world.env.program_id),
        instructions.len() - 1
    );
    assert_eq!(successes(&failure.meta, spl_token::ID), transfers);
    assert_eq!(
        world.frame(),
        before,
        "stock, receipt, SPL, identity and rent rollback"
    );
    world.custody();
    evidence.rollbacks += 1;
    evidence.paid_rollbacks += usize::from(transfers != 0);
    evidence.gate_rejections +=
        usize::from(expected == InstructionError::Custom(PercolatorError::ExpectedSigner as u32));
}

fn apply(
    world: &mut World,
    evidence: &mut Evidence,
    oracle: &mut Prefix,
    action: usize,
    ix: &Instruction,
    owners: &[usize],
) {
    let before = world.frame();
    let vault = world.env.token_amount(world.env.vault);
    let due = oracle.apply(action);
    match submit(world, evidence, &[ix.clone()], owners) {
        Ok(meta) => {
            assert_eq!(successes(&meta, world.env.program_id), 1);
            assert_eq!(successes(&meta, spl_token::ID), usize::from(due != 0));
        }
        Err(failure) => {
            assert_ne!(action, NORMALIZE);
            assert_ne!(ix.data, ProgInstruction::ClaimResolvedPayoutTopup.encode());
            assert_eq!(due, 0);
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                )
            );
            assert_eq!(world.frame(), before);
        }
    }
    assert_eq!(
        u128::from(vault - world.env.token_amount(world.env.vault)),
        due
    );
    let actor = if action == NORMALIZE {
        2
    } else {
        CLAIMANTS[action]
    };
    let mut allowed = vec![world.env.market, world.actors[actor].portfolio];
    if due != 0 {
        allowed.extend([world.env.vault, world.actors[actor].token]);
    }
    world.assert_frame_except(&before, &allowed);
    oracle.check(world);
}

#[test]
fn v16_program_partial_receipts_cross_expiry_and_owner_exit_signature_boundaries() {
    let mut evidence = Evidence::default();
    let mut peak_cu = 0;
    let mut worlds = 0;
    let mut endpoint = None;
    let configurations: [(u64, fn(&mut V16CuEnv)); 3] = [
        (1, configure::<1>),
        (2, configure::<2>),
        (5, configure::<5>),
    ];
    for (delay, setup) in configurations {
        for landing in [13, 17] {
            for crank in [false, true] {
                for order in ORDERS {
                    let mut world = World::before_receipts_with_setup(setup);
                    assert_eq!(world.env.market_state().0.force_close_delay_slots, delay);
                    let boundary = 12 + delay;
                    world
                        .land(
                            &[spl_token::instruction::set_authority(
                                &spl_token::ID,
                                &world.env.mint,
                                None,
                                spl_token::instruction::AuthorityType::MintTokens,
                                &world.env.admin.pubkey(),
                                &[],
                            )
                            .unwrap()],
                            true,
                        )
                        .unwrap();
                    for actor in CLAIMANTS {
                        let ix = route(&world, actor, crank, true);
                        for _ in 0..8 {
                            if world.receipt(actor).present {
                                break;
                            }
                            submit(&mut world, &mut evidence, &[ix.clone()], &[actor]).unwrap();
                        }
                        assert!(world.receipt(actor).present);
                    }
                    let mut oracle = Prefix::new(&world);
                    oracle.check(&world);
                    let unsigned = [0, 4, 2].map(|a| route(&world, a, crank, false));
                    let signed = [0, 4, 2].map(|a| route(&world, a, crank, true));
                    let claims = CLAIMANTS.map(|a| world.payout(a, true));
                    let bad = Instruction {
                        program_id: solana_sdk::system_program::ID,
                        accounts: vec![],
                        data: vec![],
                    };
                    world.peak_cu = 0;

                    // Expiry - 1: both partial receipts survive an unsigned zero-due
                    // lookup, while the close alias still requires its own owner.
                    for claimant in 0..2 {
                        apply(
                            &mut world,
                            &mut evidence,
                            &mut oracle,
                            claimant,
                            &claims[claimant],
                            &[],
                        );
                        reject(
                            &mut world,
                            &mut evidence,
                            &[unsigned[claimant].clone()],
                            &[],
                            InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
                            0,
                        );
                    }
                    let before = world.frame();
                    world.env.svm.warp_to_slot(landing);
                    assert_eq!(world.frame(), before);
                    oracle.check(&world);

                    // Normalization and two actual payments execute before the failing
                    // suffix; none may consume the retained receipt in the rolled-back state.
                    reject(
                        &mut world,
                        &mut evidence,
                        &[
                            signed[NORMALIZE].clone(),
                            claims[0].clone(),
                            claims[1].clone(),
                            bad.clone(),
                        ],
                        &[2],
                        InstructionError::InvalidInstructionData,
                        2,
                    );
                    if landing < boundary {
                        reject(
                            &mut world,
                            &mut evidence,
                            &[
                                signed[NORMALIZE].clone(),
                                claims[0].clone(),
                                unsigned[1].clone(),
                            ],
                            &[2],
                            InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
                            1,
                        );
                    }
                    oracle.check(&world);

                    for action in order {
                        if action == NORMALIZE {
                            let (ix, owners): (&Instruction, &[usize]) = if landing < boundary {
                                (&signed[action], &[2])
                            } else {
                                (&unsigned[action], &[])
                            };
                            apply(&mut world, &mut evidence, &mut oracle, action, ix, owners);
                        } else if action == 0 {
                            apply(
                                &mut world,
                                &mut evidence,
                                &mut oracle,
                                action,
                                &signed[action],
                                &[0],
                            );
                        } else {
                            apply(
                                &mut world,
                                &mut evidence,
                                &mut oracle,
                                action,
                                &claims[action],
                                &[],
                            );
                        }
                    }
                    // Exchange routes for catch-up: the larger receipt now uses the
                    // signed alias and the smaller one uses unsigned ClaimResolvedPayoutTopup.
                    apply(&mut world, &mut evidence, &mut oracle, 1, &signed[1], &[4]);
                    apply(&mut world, &mut evidence, &mut oracle, 0, &claims[0], &[]);
                    assert_eq!(oracle.paid, [198, 368]);

                    if landing < boundary {
                        world.env.svm.warp_to_slot(boundary - 1);
                        reject(
                            &mut world,
                            &mut evidence,
                            &[unsigned[NORMALIZE].clone()],
                            &[],
                            InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
                            0,
                        );
                        // The signed continuation is already payable. Roll it back
                        // through both receipt cleanups, then retry unsigned at equality.
                        reject(
                            &mut world,
                            &mut evidence,
                            &[
                                signed[NORMALIZE].clone(),
                                claims[1].clone(),
                                claims[0].clone(),
                                bad.clone(),
                            ],
                            &[2],
                            InstructionError::InvalidInstructionData,
                            1,
                        );
                        oracle.check(&world);
                        world.env.svm.warp_to_slot(boundary);
                    }
                    let before = world.frame();
                    let vault = world.env.token_amount(world.env.vault);
                    let meta = submit(
                        &mut world,
                        &mut evidence,
                        &[unsigned[NORMALIZE].clone()],
                        &[],
                    )
                    .unwrap();
                    assert_eq!(successes(&meta, spl_token::ID), 1);
                    assert_eq!(
                        vault - world.env.token_amount(world.env.vault),
                        1_000 + 1_000 * 851 / 3_000
                    );
                    world.assert_frame_except(
                        &before,
                        &[
                            world.env.market,
                            world.env.vault,
                            world.actors[2].portfolio,
                            world.actors[2].token,
                        ],
                    );
                    for claimant in 0..2 {
                        let mut expected = oracle.identities[claimant].receipt;
                        expected.paid_effective = oracle.paid[claimant];
                        assert_eq!(world.receipt(CLAIMANTS[claimant]), expected);
                    }
                    let ledger = world.env.market_state().1.resolved_payout_ledger;
                    assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                    assert_eq!(
                        ledger.terminal_claim_exact_receipts_num,
                        3_000 * BOUND_SCALE
                    );
                    assert_eq!(ledger.snapshot_residual, 851);

                    let all_claims = [0, 2, 4].map(|a| world.payout(a, true));
                    submit(&mut world, &mut evidence, &all_claims, &[]).unwrap();
                    let before = world.frame();
                    submit(&mut world, &mut evidence, &all_claims, &[]).unwrap();
                    assert_eq!(world.frame(), before, "paid receipts cannot pay twice");
                    let expected = [1_198, 0, 1_283, 0, 1_368];
                    for (actor, amount) in expected.into_iter().enumerate() {
                        assert_eq!(world.env.token_amount(world.actors[actor].token), amount);
                        assert!(!world.receipt(actor).present);
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                    }
                    for actor in order.map(|a| [0, 4, 2][a]).into_iter().chain([1, 3]) {
                        let portfolio = world.actors[actor].portfolio;
                        let before = world.frame();
                        let mut closed = world.env.svm.get_account(&portfolio).unwrap();
                        let rent = closed.lamports;
                        let market_rent = world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports;
                        let cu = world
                            .env
                            .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                        world.peak_cu = world.peak_cu.max(cu);
                        closed.lamports = 0;
                        closed.data.clear();
                        assert_eq!(world.env.svm.get_account(&portfolio), Some(closed));
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.env.market)
                                .unwrap()
                                .lamports,
                            market_rent + rent
                        );
                        world.assert_frame_except(&before, &[world.env.market, portfolio]);
                        world.custody();
                    }
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.materialized_portfolio_count,
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.source_claim_bound_total_num,
                            group.insurance,
                            group.vault
                        ),
                        (0, 0, 0, 0, 0, 2)
                    );
                    let result = (
                        group.resolved_payout_ledger,
                        group.source_credit,
                        group.source_backing_buckets,
                        expected,
                    );
                    assert_eq!(
                        *endpoint.get_or_insert(result.clone()),
                        result,
                        "delay, landing, alias and ordering preserve final economics"
                    );
                    peak_cu = peak_cu.max(world.peak_cu);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 72);
    assert_eq!(
        (
            evidence.rollbacks,
            evidence.paid_rollbacks,
            evidence.gate_rejections
        ),
        (288, 120, 192)
    );
    println!("INV-067 receipt exit window: {worlds} histories; {} rollbacks ({} paid), {} signature gates; peak {peak_cu} CU, {} bytes",
        evidence.rollbacks, evidence.paid_rollbacks, evidence.gate_rejections, evidence.peak_bytes);
}
