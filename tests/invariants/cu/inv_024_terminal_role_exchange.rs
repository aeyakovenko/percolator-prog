//! INV-024 / row 429: exchanging funded backing and insurance roles transfers
//! only each role's unpaid reserves, regardless of payout/handoff interleaving.
//! Public earned-fee construction, keeper payouts and separate attribution ledgers;
//! bounded resolved histories, without expiry or a generic stock/replay claim.

use super::*;

const ORIGINAL: [usize; 2] = [2, 4];
const NEXT: [usize; 2] = [4, 2];
const PRINCIPAL_PREFIX: u64 = 101;

fn reserve(
    world: &TerminalEarningsWorld,
    role: usize,
    actor: usize,
    amount: u64,
    epoch: u64,
    ledger: Pubkey,
) -> Instruction {
    let mut ix = payout(world, role, actor, amount, epoch, ledger);
    ix.accounts[0].is_signer = false;
    if role == INSURER {
        ix.accounts.push(AccountMeta::new(ledger, false));
    }
    ix
}

#[test]
fn v16_program_terminal_role_exchange_preserves_reserves_across_payout_handoff_orders() {
    // Payout precedes transfer within each role; all six linear extensions
    // commute it with the other role's payout and funded transfer.
    const ORDERS: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [0, 2, 1, 3],
        [0, 2, 3, 1],
        [2, 3, 0, 1],
        [2, 0, 3, 1],
        [2, 0, 1, 3],
    ];
    let mut maxima = [0; 4];
    let mut rollbacks = 0;
    let mut outcomes = Vec::new();
    for order in ORDERS {
        for first_tail in [FEES, INSURER] {
            let mut world = terminal_earnings_world();
            let ledgers = std::array::from_fn::<_, 4, _>(|index| {
                let key = Keypair::new();
                let len = if index < 2 {
                    state::backing_domain_ledger_account_len()
                } else {
                    state::insurance_ledger_account_len()
                };
                system_create_account_for_test(
                    &mut world.env.svm,
                    &world.env.payer,
                    &key,
                    len,
                    world.env.program_id,
                );
                key.pubkey()
            });
            let empty_ledgers = ledgers.map(|key| world.env.svm.get_account(&key).unwrap());
            let token_frames = world
                .tokens
                .map(|key| world.env.svm.get_account(&key).unwrap());
            let original_profile = state::read_asset_oracle_profile(
                &world.env.svm.get_account(&world.env.market).unwrap().data,
                0,
            )
            .unwrap();
            let original_sequences = world.env.control_sequences(0);
            let original_group = world.env.market_state().1;
            let mut expected = Entitlements {
                remaining: [EARNINGS, INSURANCE],
                paid: [PAYOUTS[0], PAYOUTS[1], 0, 0, 0],
                holders: ORIGINAL,
                principal: BACKING,
                rotations: 0,
            };
            let mut withdrawn = [0u64; 4];
            let mut observed = [None; 4];
            let check = |world: &TerminalEarningsWorld,
                         expected: &Entitlements,
                         withdrawn: [u64; 4],
                         observed: [Option<u64>; 4]| {
                expected.check(world);
                let mut profile = original_profile;
                profile.backing_bucket_authority = world.wallets[expected.holders[FEES]].to_bytes();
                profile.insurance_authority = world.wallets[expected.holders[INSURER]].to_bytes();
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &world.env.svm.get_account(&world.env.market).unwrap().data,
                        0,
                    )
                    .unwrap(),
                    profile
                );
                let mut sequences = original_sequences;
                sequences.authority_epoch += expected.rotations;
                assert_eq!(world.env.control_sequences(0), sequences);
                let group = world.env.market_state().1;
                assert_eq!(
                    group.source_backing_buckets[1].consumed_liened_backing_num,
                    u128::from(PROFIT) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_backing_buckets[1].impaired_liened_backing_num,
                    0
                );
                for domain in 0..group.source_credit.len() {
                    if domain != 1 {
                        assert_eq!(
                            group.source_credit[domain],
                            original_group.source_credit[domain]
                        );
                        assert_eq!(
                            group.source_backing_buckets[domain],
                            original_group.source_backing_buckets[domain]
                        );
                    }
                }
                for (index, token) in world.tokens.into_iter().enumerate() {
                    let mut frame = token_frames[index].clone();
                    let mut state = TokenAccount::unpack(&frame.data).unwrap();
                    state.amount = expected.paid[index];
                    TokenAccount::pack(state, &mut frame.data).unwrap();
                    assert_eq!(world.env.svm.get_account(&token), Some(frame));
                }
                for index in 0..4 {
                    let account = world.env.svm.get_account(&ledgers[index]).unwrap();
                    let Some(remaining) = observed[index] else {
                        assert_eq!(account, empty_ledgers[index]);
                        continue;
                    };
                    assert_eq!(account.lamports, empty_ledgers[index].lamports);
                    assert_eq!(account.owner, world.env.program_id);
                    let role = index / 2;
                    let actor = if index % 2 == 0 {
                        ORIGINAL[role]
                    } else {
                        NEXT[role]
                    };
                    if role == FEES {
                        assert_eq!(
                            state::read_backing_domain_ledger(&account.data).unwrap(),
                            state::BackingDomainLedgerAccountV16 {
                                market_group: world.env.market.to_bytes(),
                                authority: world.wallets[actor].to_bytes(),
                                domain: 1,
                                total_earnings_withdrawn_atoms: withdrawn[index].into(),
                                last_observed_bucket_earnings_atoms: remaining.into(),
                                last_observed_unavailable_principal_atoms: PROFIT.into(),
                                ..Default::default()
                            }
                        );
                    } else {
                        assert_eq!(
                            state::read_insurance_ledger(&account.data).unwrap(),
                            state::InsuranceLedgerAccountV16 {
                                market_group: world.env.market.to_bytes(),
                                authority: world.wallets[actor].to_bytes(),
                                total_withdrawn_atoms: withdrawn[index].into(),
                                last_observed_insurance_atoms: remaining.into(),
                                ..Default::default()
                            }
                        );
                    }
                }
            };
            let mut execute =
                |world: &mut TerminalEarningsWorld,
                 ixs: &[Instruction],
                 rejection: Option<(u8, PercolatorError, usize)>| {
                    let category = if rejection.is_some() {
                        rollbacks += 1;
                        2
                    } else {
                        usize::from(ixs.len() > 1)
                    };
                    maxima[category] = maxima[category].max(land(world, &ledgers, ixs, rejection));
                };
            check(&world, &expected, withdrawn, observed);
            let mut epoch = original_sequences.authority_epoch;
            let ix = reserve(
                &world,
                PRINCIPAL,
                ORIGINAL[FEES],
                PRINCIPAL_PREFIX,
                epoch,
                ledgers[0],
            );
            execute(&mut world, &[ix], None);
            expected.pay(PRINCIPAL, ORIGINAL[FEES], PRINCIPAL_PREFIX);
            check(&world, &expected, withdrawn, observed);

            for action in order {
                let role = action / 2;
                let transferring = action % 2 == 1;
                let index = 2 * role + usize::from(transferring);
                let amount = if transferring { 1 } else { PREFIX[role] };
                let actor = if transferring {
                    NEXT[role]
                } else {
                    ORIGINAL[role]
                };
                if transferring {
                    let handoff = rotate(&world, role, ORIGINAL[role], actor, epoch);
                    let payment = reserve(&world, role, actor, amount, epoch + 1, ledgers[index]);
                    let old_role = reserve(
                        &world,
                        role,
                        ORIGINAL[role],
                        1,
                        epoch + 1,
                        ledgers[2 * role],
                    );
                    execute(
                        &mut world,
                        &[handoff.clone(), payment.clone(), old_role],
                        Some((4, PercolatorError::Unauthorized, 1)),
                    );
                    check(&world, &expected, withdrawn, observed);
                    // Retry the identical public prefix after full role/payment/ledger rollback.
                    execute(&mut world, &[handoff, payment], None);
                    expected.holders[role] = actor;
                    expected.rotations += 1;
                    epoch += 1;
                } else {
                    let ix = reserve(&world, role, actor, amount, epoch, ledgers[index]);
                    execute(&mut world, &[ix], None);
                }
                expected.pay(role, actor, amount);
                withdrawn[index] += amount;
                observed[index] = Some(expected.remaining[role]);
                check(&world, &expected, withdrawn, observed);
            }
            assert_eq!(expected.holders, NEXT);
            // Each role's old-holder ledger remains bound to that recipient,
            // even when the recipient now owns the other role.
            for role in [FEES, INSURER] {
                let peer = 1 - role;
                let prefix = reserve(&world, peer, NEXT[peer], 1, epoch, ledgers[2 * peer + 1]);
                let wrong_record = reserve(&world, role, NEXT[role], 1, epoch, ledgers[2 * role]);
                execute(
                    &mut world,
                    &[prefix.clone(), wrong_record],
                    Some((3, PercolatorError::Unauthorized, 1)),
                );
                check(&world, &expected, withdrawn, observed);
                execute(&mut world, &[prefix], None);
                expected.pay(peer, NEXT[peer], 1);
                withdrawn[2 * peer + 1] += 1;
                observed[2 * peer + 1] = Some(expected.remaining[peer]);
                check(&world, &expected, withdrawn, observed);
            }
            for role in [first_tail, 1 - first_tail] {
                let amount = expected.remaining[role];
                let mut ixs = vec![reserve(
                    &world,
                    role,
                    NEXT[role],
                    amount,
                    epoch,
                    ledgers[2 * role + 1],
                )];
                if role == FEES {
                    ixs.push(reserve(
                        &world,
                        PRINCIPAL,
                        NEXT[role],
                        expected.principal,
                        epoch,
                        ledgers[1],
                    ));
                }
                execute(&mut world, &ixs, None);
                expected.pay(role, NEXT[role], amount);
                withdrawn[2 * role + 1] += amount;
                observed[2 * role + 1] = Some(0);
                if role == FEES {
                    expected.pay(PRINCIPAL, NEXT[role], expected.principal);
                }
                check(&world, &expected, withdrawn, observed);
            }
            assert_eq!(
                withdrawn,
                [
                    PREFIX[FEES],
                    EARNINGS - PREFIX[FEES],
                    PREFIX[INSURER],
                    INSURANCE - PREFIX[INSURER]
                ]
            );
            assert_eq!(
                expected.paid,
                [
                    PAYOUTS[0],
                    PAYOUTS[1],
                    PRINCIPAL_PREFIX + PREFIX[FEES] + INSURANCE - PREFIX[INSURER],
                    0,
                    BACKING - PRINCIPAL_PREFIX + EARNINGS - PREFIX[FEES] + PREFIX[INSURER]
                ]
            );
            assert_eq!((expected.principal, expected.remaining), (0, [0, 0]));
            outcomes.push((expected.paid, withdrawn, observed));

            let env = &world.env;
            let final_tokens = world.tokens.map(|key| env.svm.get_account(&key));
            let mut admin_frame = env.svm.get_account(&world.admin.pubkey()).unwrap();
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            admin_frame.lamports += env.svm.get_account(&env.market).unwrap().lamports
                + env.svm.get_account(&env.vault).unwrap().lamports
                - rent;
            let close = wrap(
                env,
                ProgInstruction::CloseSlab {
                    authority_epoch: epoch,
                },
                vec![
                    AccountMeta::new(world.admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(world.tokens[4], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            maxima[3] = maxima[3].max(land(&mut world, &ledgers, &[close], None));
            assert_eq!(
                world.env.svm.get_account(&world.admin.pubkey()),
                Some(admin_frame)
            );
            let slab = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&slab);
            assert_eq!(slab.lamports, rent);
            assert_eq!(
                world.tokens.map(|key| world.env.svm.get_account(&key)),
                final_tokens
            );
            assert_eq!(
                world.env.svm.get_account(&world.env.mint),
                Some(world.mint_frame.clone())
            );
            if let Some(vault) = world.env.svm.get_account(&world.env.vault) {
                assert_eq!(vault.lamports, 0);
                assert!(vault.data.is_empty());
            }
            eprintln!(
                "row429 order={order:?} first_tail={first_tail}: paid={:?}, ledgers={withdrawn:?}",
                expected.paid
            );
        }
    }
    assert_eq!(outcomes.len(), 12);
    assert!(outcomes.iter().all(|outcome| outcome == &outcomes[0]));
    assert_eq!(rollbacks, 48);
    eprintln!("INV-024 role exchange: worlds=12, exact_rollbacks={rollbacks}, CU maxima [single, bundle, rollback, close]={maxima:?}, limit={CU_LIMIT}");
}
