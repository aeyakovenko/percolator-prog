//! INV-024/036/041/070/073/080/081: spent terminal payouts survive custody
//! recreation across a funded beneficiary merge/return and a rejected paid suffix.

use super::*;

fn unsigned_payment(
    world: &TerminalEarningsWorld,
    role: usize,
    actor: usize,
    amount: u64,
    epoch: u64,
    ledger: Pubkey,
) -> Instruction {
    let mut ix = payout(world, role, actor, amount, epoch, ledger);
    ix.accounts[0] = AccountMeta::new_readonly(world.wallets[actor], false);
    if role == INSURER {
        ix.accounts.push(AccountMeta::new(ledger, false));
    }
    assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
    ix
}

#[test]
fn v16_program_missing_fee_successor_wallet_preserves_paid_history_and_unsigned_tail() {
    let mut world = terminal_earnings_world();
    let ledgers = [0; 2].map(|_| {
        let key = Keypair::new();
        system_create_account_for_test(
            &mut world.env.svm,
            &world.env.payer,
            &key,
            state::backing_domain_ledger_account_len(),
            world.env.program_id,
        );
        key.pubkey()
    });
    let epoch = world.env.control_sequences(0).authority_epoch;
    let mut book = Entitlements {
        remaining: [EARNINGS, INSURANCE],
        paid: [PAYOUTS[0], PAYOUTS[1], 0, 0, 0],
        holders: [2, 4],
        principal: BACKING,
        rotations: 0,
    };
    book.check(&world);
    let prefix = [
        payout(&world, PRINCIPAL, 2, BACKING, epoch, ledgers[0]),
        payout(&world, FEES, 2, 17, epoch, ledgers[0]),
    ];
    let mut peak = land(&mut world, &ledgers, &prefix, None);
    book.pay(PRINCIPAL, 2, BACKING);
    book.pay(FEES, 2, 17);
    book.check(&world);
    let old_ledger = world.env.svm.get_account(&ledgers[0]).unwrap();
    let old_record = state::read_backing_domain_ledger(&old_ledger.data).unwrap();
    assert_eq!(old_record.authority, world.wallets[2].to_bytes());
    assert_eq!(old_record.total_earnings_withdrawn_atoms, 17);
    assert_eq!(
        old_record.last_observed_bucket_earnings_atoms,
        u128::from(EARNINGS - 17)
    );

    let handoff = [
        rotate(&world, FEES, 2, 3, epoch),
        payout(&world, FEES, 3, 19, epoch + 1, ledgers[1]),
    ];
    peak = peak.max(land(&mut world, &ledgers, &handoff, None));
    book.holders[FEES] = 3;
    book.pay(FEES, 3, 19);
    book.check(&world);
    let first = unsigned_payment(&world, FEES, 3, 5, epoch + 1, ledgers[1]);
    let wrong_ledger = unsigned_payment(&world, FEES, 3, 1, epoch + 1, ledgers[0]);
    let tail = unsigned_payment(&world, FEES, 3, EARNINGS - 41, epoch + 1, ledgers[1]);
    let TerminalEarningsWorld {
        mut env,
        incumbent,
        successor,
        wallets,
        tokens,
        portfolios,
        mint_frame,
        ..
    } = world;
    let tracked = [env.market, env.vault, env.mint]
        .into_iter()
        .chain(wallets)
        .chain(tokens)
        .chain(portfolios)
        .chain(ledgers)
        .collect::<Vec<_>>();
    let keeper = env.payer.pubkey();
    assert!(!wallets.contains(&keeper));
    assert!(![wallets[2], wallets[3]].contains(&env.admin.pubkey()));
    for signer in [&incumbent, &successor] {
        let wallet = signer.pubkey();
        let balance = env.svm.get_account(&wallet).unwrap().lamports;
        peak = peak.max(terminal_reserve_destination_recovery::land(
            &mut env,
            &[system_instruction::transfer(&wallet, &keeper, balance)],
            &[signer],
            &tracked,
            &[wallet],
            0,
            Some((keeper, balance)),
            None,
        ));
        assert!(env.svm.get_account(&wallet).is_none_or(|account| {
            account.lamports == 0
                && account.data.is_empty()
                && account.owner == solana_sdk::system_program::ID
                && !account.executable
        }));
    }
    drop((incumbent, successor));
    let wallet_frames = wallets.map(|key| env.svm.get_account(&key));
    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let profile = state::read_asset_oracle_profile(&market_frame.data, 0).unwrap();
    assert_eq!(profile.backing_bucket_authority, wallets[3].to_bytes());
    let sequences = env.control_sequences(0);
    assert_eq!(sequences.authority_epoch, epoch + 1);
    let successor_ledger = env.svm.get_account(&ledgers[1]).unwrap();
    let record = state::read_backing_domain_ledger(&successor_ledger.data).unwrap();
    assert_eq!(record.market_group, env.market.to_bytes());
    assert_eq!(record.authority, wallets[3].to_bytes());
    assert_eq!(record.domain, 1);
    assert_eq!(record.total_earnings_atoms, 0);
    assert_eq!(record.total_earnings_withdrawn_atoms, 19);
    assert_eq!(
        record.last_observed_bucket_earnings_atoms,
        u128::from(EARNINGS - 36)
    );
    let check = |env: &V16CuEnv, successor_paid: u64, insurance_paid: u64| {
        let remaining = EARNINGS - 17 - successor_paid;
        let stock = remaining + INSURANCE - insurance_paid;
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.backing_provider_earnings_total, remaining.into());
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            remaining.into()
        );
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            0
        );
        assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);
        assert_eq!(group.insurance, u128::from(INSURANCE - insurance_paid));
        assert_eq!(group.insurance_domain_budget[0], group.insurance);
        assert!(group.insurance_domain_budget[1..]
            .iter()
            .all(|amount| *amount == 0));
        assert!(group
            .insurance_domain_spent
            .iter()
            .all(|amount| *amount == 0));
        assert_eq!(group.vault, stock.into());
        let balances = [
            PAYOUTS[0],
            PAYOUTS[1],
            BACKING + 17,
            successor_paid,
            insurance_paid,
        ];
        assert_eq!(balances.iter().sum::<u64>() + stock, SUPPLY);
        for ((key, frame), amount) in tokens
            .into_iter()
            .zip(&token_frames)
            .zip(balances)
            .chain([((env.vault, &vault_frame), stock)])
        {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), wallet_frames);
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
        assert_eq!(env.svm.get_account(&ledgers[0]), Some(old_ledger.clone()));
        let account = env.svm.get_account(&ledgers[1]).unwrap();
        let mut expected = record;
        expected.total_earnings_withdrawn_atoms = successor_paid.into();
        expected.last_observed_bucket_earnings_atoms = remaining.into();
        assert_eq!(
            state::read_backing_domain_ledger(&account.data).unwrap(),
            expected
        );
        assert_eq!(account.lamports, successor_ledger.lamports);
        let market = env.svm.get_account(&env.market).unwrap();
        assert_eq!(market.lamports, market_frame.lamports);
        assert_eq!(
            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
            profile
        );
        let mut expected_sequences = sequences;
        expected_sequences.authority_epoch += u64::from(insurance_paid != 0);
        assert_eq!(env.control_sequences(0), expected_sequences);
        crate::support::fuzz_model::assert_market_stock_census(
            "missing fee successor wallet",
            &group,
            &market.data,
            &[],
            stock.into(),
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "missing fee successor wallet",
            &group,
            &[],
        )
        .unwrap();
    };
    check(&env, 19, 0);
    // The successful fee prefix must roll back despite both beneficiary wallets being absent.
    peak = peak.max(terminal_reserve_destination_recovery::land(
        &mut env,
        &[first.clone(), wrong_ledger],
        &[],
        &tracked,
        &[],
        0,
        None,
        Some((3, PercolatorError::Unauthorized)),
    ));
    check(&env, 19, 0);
    let allowed = [env.market, env.vault, tokens[3], ledgers[1]];
    peak = peak.max(terminal_reserve_destination_recovery::land(
        &mut env,
        &[first],
        &[],
        &tracked,
        &allowed,
        0,
        None,
        None,
    ));
    check(&env, 24, 0);
    let insurance =
        terminal_public_reserves::reserve_payout(&env, wallets, tokens, ledgers[1], 2, INSURANCE);
    assert!(insurance.accounts.iter().all(|meta| !meta.is_signer));
    let allowed = [env.market, env.vault, tokens[3], tokens[4], ledgers[1]];
    peak = peak.max(terminal_reserve_destination_recovery::land(
        &mut env,
        &[tail, insurance],
        &[],
        &tracked,
        &allowed,
        0,
        None,
        None,
    ));
    check(&env, EARNINGS - 17, INSURANCE);
    assert_cu_within("missing fee successor wallet", peak, 600_000);
    eprintln!("missing fee successor wallet: worlds=1, absent_wallets=2, exact_rollbacks=1, payouts=6, peak_CU={peak}");
}

#[test]
fn v16_program_recreated_reserve_beneficiary_return_preserves_spent_prefix_and_rollback() {
    let mut peak = 0;
    let mut rollbacks = 0;
    let mut payments = 0;
    for moved in [FEES, INSURER] {
        for repair_first in [false, true] {
            let mut world = terminal_earnings_world();
            let actors = [2, 4];
            let original = actors[moved];
            let merged = actors[1 - moved];
            let debit = u64::from(moved == INSURER);
            let ledgers = [
                state::backing_domain_ledger_account_len(),
                state::insurance_ledger_account_len(),
            ]
            .map(|len| {
                [0; 2].map(|_| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut world.env.svm,
                        &world.env.payer,
                        &key,
                        len,
                        world.env.program_id,
                    );
                    key.pubkey()
                })
            });
            let spent = create_ata_for_test(
                &mut world.env.svm,
                &world.env.payer,
                Pubkey::new_unique(),
                world.env.mint,
            );
            let tracked = [world.env.market, world.env.vault, world.env.mint, spent]
                .into_iter()
                .chain(world.wallets)
                .chain(world.tokens)
                .chain(world.portfolios)
                .chain(ledgers.into_iter().flatten())
                .collect::<Vec<_>>();
            let token_frames = world
                .tokens
                .map(|key| world.env.svm.get_account(&key).unwrap());
            let spent_frame = world.env.svm.get_account(&spent).unwrap();
            let vault_frame = world.env.svm.get_account(&world.env.vault).unwrap();
            let profile = state::read_asset_oracle_profile(
                &world.env.svm.get_account(&world.env.market).unwrap().data,
                0,
            )
            .unwrap();
            let sequences = world.env.control_sequences(0);
            let rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let mut epoch = sequences.authority_epoch;
            let mut remaining = [EARNINGS, INSURANCE];
            let mut paid = [[0u64; 2]; 2];
            let mut observed = [[None; 2]; 2];
            let mut holders = actors;
            let mut principal = BACKING;
            let check = |world: &TerminalEarningsWorld,
                         remaining: [u64; 2],
                         paid: [[u64; 2]; 2],
                         observed: [[Option<u64>; 2]; 2],
                         holders: [usize; 2],
                         principal: u64,
                         epoch: u64,
                         spent_amount: u64,
                         present: bool| {
                let env = &world.env;
                let (cfg, group) = env.market_state();
                assert_eq!(cfg.marketauth, world.admin.pubkey().to_bytes());
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(
                    group.backing_provider_earnings_total,
                    remaining[FEES].into()
                );
                assert_eq!(
                    group.source_backing_buckets[1].utilization_fee_earnings,
                    remaining[FEES].into()
                );
                assert_eq!(
                    group.source_backing_buckets[1].fresh_unliened_backing_num,
                    u128::from(principal) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_credit[1].fresh_reserved_backing_num,
                    u128::from(principal) * BOUND_SCALE
                );
                assert_eq!(group.insurance, remaining[INSURER].into());
                assert_eq!(group.insurance_domain_budget[0], group.insurance);
                assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
                assert!(group.insurance_domain_spent.iter().all(|v| *v == 0));
                let stock = principal + remaining.iter().sum::<u64>();
                assert_eq!(group.vault, stock.into());
                let mut balances = [PAYOUTS[0], PAYOUTS[1], BACKING - principal, 0, 0];
                for owner in 0..2 {
                    balances[actors[owner]] += paid[FEES][owner] + paid[INSURER][owner];
                }
                balances[original] -= spent_amount;
                assert_eq!(balances.iter().sum::<u64>() + stock + spent_amount, SUPPLY);
                for (key, frame, amount) in world
                    .tokens
                    .into_iter()
                    .zip(token_frames.iter())
                    .zip(balances)
                    .map(|((key, frame), amount)| (key, frame, amount))
                    .chain([
                        (spent, &spent_frame, spent_amount),
                        (env.vault, &vault_frame, stock),
                    ])
                {
                    if key == world.tokens[original] && !present {
                        assert_eq!(amount, 0);
                        assert!(env.svm.get_account(&key).is_none_or(|a| a.lamports == 0
                            && a.data.is_empty()
                            && a.owner == solana_sdk::system_program::ID));
                    } else {
                        let mut expected = frame.clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = amount;
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(&key), Some(expected));
                    }
                }
                for role in [FEES, INSURER] {
                    for owner in 0..2 {
                        let account = env.svm.get_account(&ledgers[role][owner]).unwrap();
                        if let Some(last) = observed[role][owner] {
                            let (market, authority, withdrawn, observation) = if role == FEES {
                                let ledger =
                                    state::read_backing_domain_ledger(&account.data).unwrap();
                                assert_eq!(ledger.domain, 1);
                                assert_eq!(ledger.total_principal_withdrawn_atoms, 0);
                                (
                                    ledger.market_group,
                                    ledger.authority,
                                    ledger.total_earnings_withdrawn_atoms,
                                    ledger.last_observed_bucket_earnings_atoms,
                                )
                            } else {
                                let ledger = state::read_insurance_ledger(&account.data).unwrap();
                                (
                                    ledger.market_group,
                                    ledger.authority,
                                    ledger.total_withdrawn_atoms,
                                    ledger.last_observed_insurance_atoms,
                                )
                            };
                            assert_eq!(market, env.market.to_bytes());
                            assert_eq!(authority, world.wallets[actors[owner]].to_bytes());
                            assert_eq!(withdrawn, paid[role][owner].into());
                            assert_eq!(observation, last.into());
                        } else {
                            assert!(account.data.iter().all(|byte| *byte == 0));
                        }
                    }
                }
                let mut expected_profile = profile;
                expected_profile.backing_bucket_authority = world.wallets[holders[FEES]].to_bytes();
                expected_profile.insurance_authority = world.wallets[holders[INSURER]].to_bytes();
                let market = env.svm.get_account(&env.market).unwrap();
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    expected_profile
                );
                let mut expected_sequences = sequences;
                expected_sequences.authority_epoch = epoch;
                assert_eq!(env.control_sequences(0), expected_sequences);
                assert_eq!(
                    env.svm.get_account(&env.mint),
                    Some(world.mint_frame.clone())
                );
                crate::support::fuzz_model::assert_market_stock_census(
                    "recreated beneficiary handoff",
                    &group,
                    &market.data,
                    &[],
                    stock.into(),
                )
                .unwrap();
                crate::support::fuzz_model::assert_reservation_encumbrance_census(
                    "recreated beneficiary handoff",
                    &group,
                    &[],
                )
                .unwrap();
                let mut data = market.data;
                state::market_view_mut(&mut data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            let mut land = |world: &mut TerminalEarningsWorld,
                            ixs: &[Instruction],
                            allowed: &[Pubkey],
                            rent: u64,
                            refund: Option<(Pubkey, u64)>,
                            rejection: Option<(u8, PercolatorError)>| {
                let signers = [&world.admin, &world.incumbent]
                    .into_iter()
                    .filter(|signer| {
                        ixs.iter()
                            .flat_map(|ix| &ix.accounts)
                            .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
                    })
                    .collect::<Vec<_>>();
                if rejection.is_some() {
                    rollbacks += 1;
                }
                peak = peak.max(terminal_reserve_destination_recovery::land(
                    &mut world.env,
                    ixs,
                    &signers,
                    &tracked,
                    allowed,
                    rent,
                    refund,
                    rejection,
                ));
            };
            for role in [FEES, INSURER] {
                let ix = unsigned_payment(
                    &world,
                    role,
                    actors[role],
                    PREFIX[role],
                    epoch,
                    ledgers[role][role],
                );
                let allowed = [
                    world.env.market,
                    world.env.vault,
                    world.tokens[actors[role]],
                    ledgers[role][role],
                ];
                land(&mut world, &[ix], &allowed, 0, None, None);
                payments += 1;
                paid[role][role] += PREFIX[role];
                remaining[role] -= PREFIX[role];
                observed[role][role] = Some(remaining[role]);
                epoch += u64::from(role == INSURER);
                check(
                    &world, remaining, paid, observed, holders, principal, epoch, 0, true,
                );
            }
            let retained =
                unsigned_payment(&world, moved, original, 1, epoch, ledgers[moved][moved]);
            let spend_close = [
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &world.tokens[original],
                    &spent,
                    &world.wallets[original],
                    &[],
                    PREFIX[moved],
                )
                .unwrap(),
                spl_token::instruction::close_account(
                    &spl_token::ID,
                    &world.tokens[original],
                    &world.wallets[original],
                    &world.wallets[original],
                    &[],
                )
                .unwrap(),
            ];
            let allowed = [world.tokens[original], spent];
            let refund = Some((world.wallets[original], rent));
            land(&mut world, &spend_close, &allowed, 0, refund, None);
            check(
                &world,
                remaining,
                paid,
                observed,
                holders,
                principal,
                epoch,
                PREFIX[moved],
                false,
            );
            let handoff = rotate(&world, moved, original, merged, epoch);
            let allowed = [world.env.market];
            land(&mut world, &[handoff], &allowed, 0, None, None);
            epoch += 1;
            holders[moved] = merged;
            check(
                &world,
                remaining,
                paid,
                observed,
                holders,
                principal,
                epoch,
                PREFIX[moved],
                false,
            );
            let repair = Instruction {
                program_id: associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(world.env.payer.pubkey(), true),
                    AccountMeta::new(world.tokens[original], false),
                    AccountMeta::new_readonly(world.wallets[original], false),
                    AccountMeta::new_readonly(world.env.mint, false),
                    AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: vec![1],
            };
            // Recreating the old owner's ATA does not restore its transferred claim.
            let displaced =
                unsigned_payment(&world, moved, original, 1, epoch, ledgers[moved][moved]);
            land(
                &mut world,
                &[repair.clone(), displaced],
                &[],
                0,
                None,
                Some((
                    3,
                    if moved == FEES {
                        PercolatorError::Unauthorized
                    } else {
                        PercolatorError::InvalidTokenAccount
                    },
                )),
            );
            let middle_payment =
                unsigned_payment(&world, moved, merged, 5, epoch, ledgers[moved][1 - moved]);
            let return_role = rotate(&world, moved, merged, original, epoch + debit);
            let return_payment = unsigned_payment(
                &world,
                moved,
                original,
                3,
                epoch + debit + 1,
                ledgers[moved][moved],
            );
            let completion = if repair_first {
                vec![repair, middle_payment, return_role, return_payment]
            } else {
                vec![middle_payment, return_role, repair, return_payment]
            };
            let encoded = bincode::serialize(&completion).unwrap();
            let mut wrong = unsigned_payment(
                &world,
                moved,
                original,
                1,
                epoch + 2 * debit + 1,
                ledgers[moved][moved],
            );
            wrong.accounts[if moved == FEES { 3 } else { 2 }].pubkey = world.tokens[3];
            let mut rejected = completion.clone();
            rejected.push(wrong);
            land(
                &mut world,
                &rejected,
                &[],
                0,
                None,
                Some((6, PercolatorError::InvalidTokenAccount)),
            );
            check(
                &world,
                remaining,
                paid,
                observed,
                holders,
                principal,
                epoch,
                PREFIX[moved],
                false,
            );
            assert_eq!(bincode::serialize(&completion).unwrap(), encoded);
            let allowed = [
                world.env.market,
                world.env.vault,
                world.tokens[original],
                world.tokens[merged],
                ledgers[moved][0],
                ledgers[moved][1],
            ];
            land(&mut world, &completion, &allowed, rent, None, None);
            payments += 2;
            paid[moved][1 - moved] += 5;
            remaining[moved] -= 5;
            observed[moved][1 - moved] = Some(remaining[moved]);
            paid[moved][moved] += 3;
            remaining[moved] -= 3;
            observed[moved][moved] = Some(remaining[moved]);
            holders[moved] = original;
            epoch += 2 * debit + 1;
            check(
                &world,
                remaining,
                paid,
                observed,
                holders,
                principal,
                epoch,
                PREFIX[moved],
                true,
            );
            // Identity has returned, but the pre-handoff request remains stale.
            land(
                &mut world,
                &[retained],
                &[],
                0,
                None,
                Some((2, PercolatorError::EngineStale)),
            );
            check(
                &world,
                remaining,
                paid,
                observed,
                holders,
                principal,
                epoch,
                PREFIX[moved],
                true,
            );
            for role in [FEES, INSURER, PRINCIPAL] {
                let owner = if role == INSURER { 1 } else { 0 };
                let amount = if role == PRINCIPAL {
                    principal
                } else {
                    remaining[role]
                };
                let ledger = ledgers[usize::from(role == INSURER)][owner];
                let ix = unsigned_payment(&world, role, actors[owner], amount, epoch, ledger);
                let allowed = [
                    world.env.market,
                    world.env.vault,
                    world.tokens[actors[owner]],
                    ledger,
                ];
                land(&mut world, &[ix], &allowed, 0, None, None);
                payments += 1;
                if role == PRINCIPAL {
                    principal = 0;
                } else {
                    paid[role][owner] += amount;
                    remaining[role] = 0;
                    observed[role][owner] = Some(0);
                    epoch += u64::from(role == INSURER);
                }
                check(
                    &world,
                    remaining,
                    paid,
                    observed,
                    holders,
                    principal,
                    epoch,
                    PREFIX[moved],
                    true,
                );
            }
            let close = wrap(
                &world.env,
                ProgInstruction::CloseSlab {
                    authority_epoch: epoch,
                },
                vec![
                    AccountMeta::new(world.admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new(world.tokens[4], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(world.env.mint, false),
                ],
            );
            let tombstone_rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            let refund = world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports
                + vault_frame.lamports
                - tombstone_rent;
            let allowed = [world.env.market, world.env.vault];
            let recipient = world.admin.pubkey();
            land(
                &mut world,
                &[close],
                &allowed,
                0,
                Some((recipient, refund)),
                None,
            );
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        }
    }
    assert_eq!((rollbacks, payments), (12, 28));
    assert_cu_within("recreated beneficiary return", peak, 1_200_000);
    eprintln!("recreated beneficiary return: worlds=4, exact_rollbacks={rollbacks}, payouts={payments}, closures=4, peak_CU={peak}");
}
