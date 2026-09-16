use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const SUPPLY: u64 = 3_852;
const LIMIT: u64 = 500_000;

fn freezable_quote(env: &mut V16CuEnv) {
    let mint = Keypair::new();
    system_create_account_for_test(&mut env.svm, &env.payer, &mint, Mint::LEN, spl_token::ID);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::initialize_mint2(
            &spl_token::ID,
            &mint.pubkey(),
            &env.admin.pubkey(),
            Some(&env.admin.pubkey()),
            0,
        )
        .unwrap(),
        &[],
    )
    .unwrap();
    let admin = env.admin.insecure_clone();
    env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: mint.pubkey().to_bytes(),
            secondary_mint: env.mint.to_bytes(),
            authority_epoch: env.control_sequences(0).authority_epoch,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(mint.pubkey(), false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(env.vault, false),
        ],
        &[&admin],
    )
    .unwrap();
    env.mint = mint.pubkey();
    env.vault = create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, env.mint);
}

fn successes(meta: &litesvm::types::TransactionMetadata, program: Pubkey) -> usize {
    meta.logs
        .iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn land(
    world: &mut World,
    replacement: Pubkey,
    instructions: &[Instruction],
    changed: &[Pubkey],
    rent: u64,
    reject: Option<(u8, InstructionError)>,
    completed: (usize, usize),
) {
    world.env.svm.expire_blockhash();
    let ixs: Vec<_> = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ]
    .into_iter()
    .chain(instructions.iter().cloned())
    .collect();
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer],
        world.env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys: Vec<_> = world.frame().into_iter().map(|(key, _)| key).collect();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.extend([replacement, solana_sdk::sysvar::clock::ID]);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    let identities = |world: &World| {
        world
            .actors
            .iter()
            .map(|actor| {
                (
                    world.env.portfolio_id(actor.portfolio),
                    world.env.portfolio_position_epoch(actor.portfolio),
                    state::read_portfolio_owner_preflight(
                        &world.env.svm.get_account(&actor.portfolio).unwrap().data,
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let identity_before = identities(world);
    let result = world.env.svm.send_transaction(tx);
    let rejected = reject.is_some();
    let meta = if let Some((index, error)) = reject {
        let failure = result.expect_err("late rejection must restore receipt and custody");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, error)
        );
        failure.meta
    } else {
        result.expect("keeper-only receipt continuation")
    };
    assert_eq!(successes(&meta, world.env.program_id), completed.0);
    assert_eq!(successes(&meta, spl_token::ID), completed.1);
    assert_cu_within(
        "frozen paid-prefix receipt",
        meta.compute_units_consumed,
        LIMIT,
    );
    world.peak_cu = world.peak_cu.max(meta.compute_units_consumed);
    assert_eq!(
        identities(world),
        identity_before,
        "portfolio provenance and incarnation"
    );
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -=
                FeeStructure::default().lamports_per_signature + if rejected { 0 } else { rent };
        } else if !rejected && changed.contains(&key) {
            continue;
        }
        assert_eq!(
            world.env.svm.get_account(&key),
            expected,
            "Account frame {key}"
        );
    }
}

fn custody(world: &World, replacement: Pubkey) {
    let extra = world
        .env
        .svm
        .get_account(&replacement)
        .map_or(0, |account| {
            TokenAccount::unpack(&account.data).unwrap().amount
        });
    let vault = world.env.token_amount(world.env.vault);
    assert_eq!(u128::from(vault), world.env.market_state().1.vault);
    assert_eq!(
        vault
            + extra
            + world.env.token_amount(world.provider_token)
            + world
                .actors
                .iter()
                .map(|a| world.env.token_amount(a.token))
                .sum::<u64>(),
        SUPPLY
    );
    let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data).unwrap();
    assert_eq!(
        (mint.supply, mint.mint_authority, mint.freeze_authority),
        (SUPPLY, COption::None, COption::None)
    );
}

#[test]
fn v16_program_frozen_paid_receipt_prefix_preserves_keeper_only_late_expiry_topup() {
    // INV-067/078: replacing permanently frozen custody must pay only the late-expiry
    // delta, retain the frozen paid prefix, and never require owner/freezer cooperation.
    for frozen in [0, 4] {
        for repaired_first in [false, true] {
            let peer = 4 - frozen;
            let mut world = World::before_receipts_with_setup(freezable_quote);
            for actor in [0, 4] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    world.land(&[world.payout(actor, false)], false).unwrap();
                }
                let receipt = world.receipt(actor);
                assert!(receipt.present && !receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                assert_eq!(receipt.paid_effective, FACES[actor] * 501 / 3_000);
                assert_eq!(
                    u128::from(world.env.token_amount(world.actors[actor].token)),
                    1_000 + receipt.paid_effective
                );
            }
            let old_receipts = [world.receipt(frozen), world.receipt(peer)];
            let retained = world.payout(frozen, true);
            let peer_claim = world.payout(peer, true);
            let source_close = world.payout(2, false);
            let token = world.actors[frozen].token;
            let owner = world.actors[frozen].owner.pubkey();
            let freeze = spl_token::instruction::freeze_account(
                &spl_token::ID,
                &token,
                &world.env.mint,
                &world.env.admin.pubkey(),
                &[],
            )
            .unwrap();
            let mut revoke = vec![freeze];
            for authority in [
                spl_token::instruction::AuthorityType::MintTokens,
                spl_token::instruction::AuthorityType::FreezeAccount,
            ] {
                revoke.push(
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &world.env.mint,
                        None,
                        authority,
                        &world.env.admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                );
            }
            world.land(&revoke, true).unwrap();
            let frozen_frame = world.env.svm.get_account(&token).unwrap();
            assert_eq!(
                TokenAccount::unpack(&frozen_frame.data).unwrap().state,
                AccountState::Frozen
            );
            let mint_frame = world.env.svm.get_account(&world.env.mint).unwrap();
            let seed = "receipt-frozen-prefix";
            let replacement =
                Pubkey::create_with_seed(&world.env.payer.pubkey(), seed, &spl_token::ID).unwrap();
            let rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let mut repaired = retained.clone();
            repaired.accounts[3] = AccountMeta::new(replacement, false);
            let repair = vec![
                system_instruction::create_account_with_seed(
                    &world.env.payer.pubkey(),
                    &replacement,
                    &world.env.payer.pubkey(),
                    seed,
                    rent,
                    TokenAccount::LEN as u64,
                    &spl_token::ID,
                ),
                spl_token::instruction::initialize_account3(
                    &spl_token::ID,
                    &replacement,
                    &world.env.mint,
                    &owner,
                )
                .unwrap(),
                repaired.clone(),
            ];
            world.peak_cu = 0;
            world.env.svm.warp_to_slot(13);
            custody(&world, replacement);
            let market = world.env.market;
            // The obsolete destination is harmless at zero due. Normalization makes
            // the same retained request payable, so its custody must then be checked.
            land(
                &mut world,
                replacement,
                &[retained.clone()],
                &[market],
                0,
                None,
                (1, 0),
            );
            land(
                &mut world,
                replacement,
                &[source_close.clone(), peer_claim.clone(), retained.clone()],
                &[],
                0,
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32),
                )),
                (2, 1),
            );
            assert_eq!([world.receipt(frozen), world.receipt(peer)], old_receipts);
            assert_eq!(
                world.env.market_state().1.source_backing_buckets[3].status,
                BackingBucketStatusV16::Fresh
            );
            assert_eq!(
                world
                    .env
                    .market_state()
                    .1
                    .resolved_payout_ledger
                    .snapshot_residual,
                501
            );
            custody(&world, replacement);

            let source = world.actors[2].portfolio;
            land(
                &mut world,
                replacement,
                &[source_close.clone()],
                &[market, source],
                0,
                None,
                (1, 0),
            );
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(ledger.snapshot_slot, 12);
            assert_eq!(ledger.snapshot_residual, 851);
            assert_eq!(ledger.current_payout_rate_num, 851 * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
            assert_eq!(
                ledger.terminal_claim_bound_unreceipted_num,
                1_000 * BOUND_SCALE
            );
            assert_eq!(
                world.env.market_state().1.source_backing_buckets[3].status,
                BackingBucketStatusV16::Expired
            );
            let group = world.env.market_state().1;
            assert_eq!(group.source_credit[3].fresh_reserved_backing_num, 0);
            assert_eq!(
                group.source_backing_buckets[3].fresh_unliened_backing_num,
                0
            );
            assert_eq!(group.source_credit[3].provider_receivable_num, 0);
            assert_eq!(group.source_credit[3].spent_backing_num, 0);
            assert_eq!([world.receipt(frozen), world.receipt(peer)], old_receipts);

            let mut abort = repair.clone();
            abort.push(Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![],
            });
            land(
                &mut world,
                replacement,
                &abort,
                &[],
                0,
                Some((5, InstructionError::InvalidInstructionData)),
                (1, 2),
            );
            assert!(world.env.svm.get_account(&replacement).is_none());
            assert_eq!([world.receipt(frozen), world.receipt(peer)], old_receipts);
            custody(&world, replacement);

            for actor in if repaired_first {
                [frozen, peer]
            } else {
                [peer, frozen]
            } {
                let destination = if actor == frozen {
                    replacement
                } else {
                    world.actors[actor].token
                };
                let changed = [
                    market,
                    world.env.vault,
                    world.actors[actor].portfolio,
                    destination,
                ];
                let before_vault = world.env.token_amount(world.env.vault);
                let due = FACES[actor] * 851 / 3_000 - FACES[actor] * 501 / 3_000;
                assert_eq!(due, if actor == 0 { 82 } else { 151 });
                if actor == frozen {
                    land(
                        &mut world,
                        replacement,
                        &repair,
                        &changed,
                        rent,
                        None,
                        (1, 2),
                    );
                    let image = world.env.svm.get_account(&replacement).unwrap();
                    let account = TokenAccount::unpack(&image.data).unwrap();
                    assert_eq!(
                        (account.owner, account.mint, account.amount, account.state),
                        (owner, world.env.mint, due as u64, AccountState::Initialized)
                    );
                    assert_eq!(image.lamports, rent);
                } else {
                    land(
                        &mut world,
                        replacement,
                        &[peer_claim.clone()],
                        &changed,
                        0,
                        None,
                        (1, 1),
                    );
                }
                assert_eq!(
                    u128::from(before_vault - world.env.token_amount(world.env.vault)),
                    due
                );
                let mut expected = old_receipts[usize::from(actor == peer)];
                expected.paid_effective = FACES[actor] * 851 / 3_000;
                assert_eq!(world.receipt(actor), expected);
                assert_eq!(
                    world.env.svm.get_account(&token),
                    Some(frozen_frame.clone())
                );
                assert_eq!(
                    world.env.svm.get_account(&world.env.mint),
                    Some(mint_frame.clone())
                );
                custody(&world, replacement);
            }
            for replay in [retained.clone(), repaired.clone(), peer_claim.clone()] {
                land(&mut world, replacement, &[replay], &[], 0, None, (1, 0));
            }

            // Finish the remaining bound and all receipts using the same sole fee payer.
            for _ in 0..8 {
                for actor in [2, peer, frozen] {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        continue;
                    }
                    let ix = if actor == frozen {
                        repaired.clone()
                    } else if actor == peer {
                        peer_claim.clone()
                    } else {
                        source_close.clone()
                    };
                    let changed = [
                        market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        world.actors[actor].token,
                    ];
                    let before = world.frame();
                    let pays_source =
                        actor == 2 && world.env.token_amount(world.actors[2].token) == 0;
                    land(
                        &mut world,
                        replacement,
                        &[ix],
                        &changed,
                        0,
                        None,
                        (1, usize::from(pays_source)),
                    );
                    assert_ne!(world.frame(), before, "unfinished receipt must progress");
                    custody(&world, replacement);
                }
                if world
                    .actors
                    .iter()
                    .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                {
                    break;
                }
            }
            for actor in 0..5 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                let expected = if FACES[actor] == 0 {
                    0
                } else {
                    1_000 + FACES[actor] * 851 / 3_000
                };
                let extra = if actor == frozen {
                    world.env.token_amount(replacement)
                } else {
                    0
                };
                assert_eq!(
                    u128::from(world.env.token_amount(world.actors[actor].token) + extra),
                    expected
                );
            }
            for replay in [retained, repaired, peer_claim] {
                land(&mut world, replacement, &[replay], &[], 0, None, (1, 0));
            }
            let group = world.env.market_state().1;
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num,
                    group.insurance,
                    group.backing_provider_earnings_total
                ),
                (0, 0, 0, 0, 0)
            );
            assert_eq!(
                group.vault,
                851 - [0, 2, 4]
                    .map(|a| FACES[a] * 851 / 3_000)
                    .iter()
                    .sum::<u128>()
            );
            assert_eq!(group.vault, 2);
            assert_eq!(world.env.svm.get_account(&token), Some(frozen_frame));
            assert_eq!(world.env.svm.get_account(&world.env.mint), Some(mint_frame));
            custody(&world, replacement);
            println!("INV-067/078 frozen={frozen}, repaired_first={repaired_first}: 2 atomic rollbacks, exact topup only, keeper terminal exit; peak CU={}", world.peak_cu);
        }
    }
}
