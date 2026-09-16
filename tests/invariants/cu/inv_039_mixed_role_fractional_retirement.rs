//! INV-039/024: simultaneous creditor/debtor roles retain their input-derived
//! entitlements through fractional peer-source conversion and backing expiry.
//! The idle provider has no trading or insurance role. No ADL/recredit claim.
//! The native suffix instead retires source-support discounts to an independent
//! asset-0 insurer, separating owner entitlements, residue, donated surplus and rent.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_039_mixed_role_close_preemption.rs"]
mod close_preemption;

#[path = "inv_039_mixed_role_unsettled_expiry.rs"]
mod unsettled_expiry;

#[path = "inv_039_mixed_underfunded_receipt.rs"]
mod underfunded_receipt;

#[path = "inv_039_mixed_role_adl_rebalance.rs"]
mod adl_rebalance;

const EXPIRY: u64 = 25;
// Debt crystallizes at slot 15; the setup's largest freshness horizon is 1,000.
const SOURCE_EXPIRY: u64 = 1_015;

#[track_caller]
fn land(
    world: &mut AttributionWorld,
    instructions: &[Instruction],
    signers: &[&Keypair],
    changed: &[Pubkey],
    rejected_prefix: Option<usize>,
) -> u64 {
    world.env.svm.expire_blockhash();
    let mut batch = vec![heap_ix(), cu_ix()];
    batch.extend_from_slice(instructions);
    let mut signing = vec![&world.env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &batch,
        Some(&world.env.payer.pubkey()),
        &signing,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        1 + signers.len()
    );
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    let lamports_before: u128 = before.iter().flatten().map(|a| a.lamports as u128).sum();
    let result = world.env.svm.send_transaction(tx);
    let meta = if let Some(prefix) = rejected_prefix {
        let failure = result.expect_err("unsigned deletion suffix");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (2 + prefix) as u8,
                InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
            )
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", world.env.program_id))
                .count(),
            instructions[..prefix]
                .iter()
                .filter(|ix| ix.program_id == world.env.program_id)
                .count()
        );
        failure.meta
    } else {
        result.expect("bounded mixed-role continuation")
    };
    assert_eq!(
        keys.iter()
            .filter_map(|key| world.env.svm.get_account(key))
            .map(|a| a.lamports as u128)
            .sum::<u128>(),
        lamports_before - fee as u128,
        "complete transaction lamport conservation"
    );
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if rejected_prefix.is_some() || !changed.contains(&key) {
            assert_eq!(
                world.env.svm.get_account(&key),
                expected,
                "complete Account {key}"
            );
        }
    }
    assert_cu_within(
        "mixed fractional terminal transaction",
        meta.compute_units_consumed,
        600_000,
    );
    meta.compute_units_consumed
}

fn payout(world: &AttributionWorld, actor: usize, claim: bool) -> Instruction {
    let env = &world.env;
    let a = &world.actors[actor];
    Instruction {
        program_id: env.program_id,
        data: if claim {
            ProgInstruction::ClaimResolvedPayoutTopup
        } else {
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
        }
        .encode(),
        accounts: vec![
            AccountMeta::new_readonly(a.owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a.portfolio, false),
            AccountMeta::new(a.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    }
}

fn deletion(world: &AttributionWorld, actor: usize, signed: bool) -> Instruction {
    Instruction {
        program_id: world.env.program_id,
        data: world
            .env
            .close_portfolio_ix(world.actors[actor].portfolio)
            .encode(),
        accounts: vec![
            AccountMeta::new(world.actors[actor].owner.pubkey(), signed),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.actors[actor].portfolio, false),
        ],
    }
}

fn fund_provider(world: &mut AttributionWorld, book: &mut Book, amount: u128) -> u64 {
    let admin = world.env.admin.insecure_clone();
    let incoming = world.actors[4].owner.insecure_clone();
    let before = world.frame();
    let role_cu = world
        .env
        .try_update_per_asset_authority_with_cu(
            &admin,
            Some(&incoming),
            0,
            processor::ASSET_AUTH_BACKING_BUCKET,
            incoming.pubkey().to_bytes(),
        )
        .unwrap();
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    book.check(world);
    let env = &world.env;
    let provider = &world.actors[4];
    let owner = provider.owner.insecure_clone();
    let market = env.market;
    let vault = env.vault;
    let portfolio = provider.portfolio;
    let token = provider.token;
    let withdraw = Instruction {
        program_id: env.program_id,
        data: env.withdraw_ix(portfolio, amount).encode(),
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    };
    let fund = Instruction {
        program_id: env.program_id,
        data: ProgInstruction::TopUpBackingBucket {
            domain: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            intent_id: next_control_sequence(env.control_sequences(0).backing_top_up),
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount,
            expiry_slot: EXPIRY,
        }
        .encode(),
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    };
    let cu = land(
        world,
        &[withdraw, fund],
        &[&owner],
        &[market, portfolio, token, vault],
        None,
    );
    book.provider_principal = amount;
    book.check(world);
    assert_eq!(
        world.env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num,
        amount * BOUND_SCALE
    );
    assert_eq!(world.env.token_amount(token), 0);
    cu.max(role_cu)
}

fn terminal_check(world: &AttributionWorld, payouts: [u128; 5], reserve: u128, expired: bool) {
    let env = &world.env;
    let group = env.market_state().1;
    for (actor, expected) in world.actors.iter().zip(payouts) {
        let token = TokenAccount::unpack(&env.svm.get_account(&actor.token).unwrap().data).unwrap();
        assert_eq!(
            (token.owner, token.mint, token.amount as u128),
            (actor.owner.pubkey(), env.mint, expected)
        );
    }
    assert_eq!(
        (
            group.c_tot,
            group.pnl_pos_tot,
            group.materialized_portfolio_count
        ),
        (0, 0, 0)
    );
    assert_eq!(
        group.insurance, 0,
        "no historic insurance spend can be recredited"
    );
    assert_eq!(
        group.source_backing_buckets[0].fresh_unliened_backing_num,
        if expired { 0 } else { reserve * BOUND_SCALE }
    );
    let vault = env.token_amount(env.vault) as u128;
    let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
        .unwrap()
        .supply as u128;
    assert_eq!(vault + payouts.into_iter().sum::<u128>(), supply);
    assert_eq!(supply, DEPOSITS.into_iter().sum::<u128>());
    assert_market_stock_census(
        "mixed fractional retirement",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &[],
        vault,
    )
    .unwrap();
    assert_reservation_encumbrance_census("mixed fractional retirement", &group, &[]).unwrap();
}

#[test]
fn v16_program_mixed_role_fractional_source_preserves_attribution_through_expiry_and_retirement() {
    let mut worlds = 0;
    let mut peak = 0;
    let mut rollbacks = 0;
    let mut waits = 0;
    let mut receipts = 0;
    let mut terminal_calls = 0;
    for (debt, backing) in [(216_000, 101), (234_000, 307)] {
        let mut normalized = None;
        for reverse in [false, true] {
            for assets in [[1, 2], [2, 1]] {
                for schedule in 0..2 {
                    for late in [false, true] {
                        let (mut world, mut book) = setup(reverse, assets, debt);
                        assert_eq!(book.support(), DEPOSITS[1]);
                        assert_eq!(
                            book.peer_receipt_face(),
                            book.support() + 1,
                            "fractional conversion retains an additional receipt atom"
                        );
                        peak = peak.max(fund_provider(&mut world, &mut book, backing));
                        if schedule == 1 {
                            world.env.crank(
                                world.actors[1].portfolio,
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: 10,
                                    observations: crank_observations(assets[0] as u16),
                                },
                            );
                            book.check(&world);
                            assert!(book.booked && !book.charged && !book.debt_settled);
                        }
                        world.env.resolve();
                        book.check(&world);
                        world.env.svm.warp_to_slot(15);
                        let market = world.env.market;
                        let vault = world.env.vault;
                        let denied = deletion(&world, 3, false);
                        let first = payout(&world, 1, false);
                        peak = peak.max(land(
                            &mut world,
                            &[first.clone(), denied.clone()],
                            &[],
                            &[],
                            Some(1),
                        ));
                        rollbacks += 1;
                        book.check(&world);
                        let changed = [
                            market,
                            vault,
                            world.actors[1].portfolio,
                            world.actors[1].token,
                        ];
                        peak = peak.max(land(&mut world, &[first], &[], &changed, None));
                        book.check(&world);

                        let order = if schedule == 0 {
                            [0, 2, 1, 4, 3]
                        } else {
                            [2, 1, 0, 3, 4]
                        };
                        for _ in 0..16 {
                            for actor in order {
                                if !resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    waits += usize::from(book.close(&mut world, actor, &mut peak));
                                }
                            }
                            if world
                                .actors
                                .iter()
                                .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                            {
                                break;
                            }
                        }
                        assert!(book.booked && book.charged && book.debt_settled);
                        let peer = world.env.portfolio_state(world.actors[2].portfolio);
                        let receipt = resolved_receipt(&peer);
                        assert!(receipt.present);
                        assert_eq!(
                            (receipt.terminal_positive_claim_face, receipt.paid_effective),
                            (book.peer_receipt_face(), book.peer_receipt_face())
                        );
                        let source_domain = assets[1] * 2 + usize::from(!reverse);
                        assert_eq!(
                            world.env.market_state().1.source_credit[source_domain]
                                .fresh_reserved_backing_num,
                            BOUND_SCALE,
                            "the conversion's remaining atom stays source-reserved"
                        );
                        assert_eq!(
                            world.env.market_state().1.source_backing_buckets[source_domain]
                                .expiry_slot,
                            SOURCE_EXPIRY
                        );
                        let retry = payout(&world, 2, true);
                        peak = peak.max(land(&mut world, &[retry], &[], &[], None));
                        book.check(&world);
                        receipts += 1;

                        let payouts = book.payouts();
                        for actor in order {
                            assert!(resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio
                            ));
                            let owner = world.actors[actor].owner.insecure_clone();
                            let portfolio = world.actors[actor].portfolio;
                            let close = deletion(&world, actor, true);
                            if actor == order[0] {
                                // A completed owner deletion cannot commit before an unsigned peer deletion.
                                let suffix = deletion(&world, order[1], false);
                                peak = peak.max(land(
                                    &mut world,
                                    &[close.clone(), suffix],
                                    &[&owner],
                                    &[],
                                    Some(1),
                                ));
                                rollbacks += 1;
                                book.check(&world);
                            }
                            let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                            let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                            peak = peak.max(land(
                                &mut world,
                                &[close],
                                &[&owner],
                                &[market, portfolio],
                                None,
                            ));
                            assert_eq!(
                                world.env.svm.get_account(&market).unwrap().lamports,
                                market_rent + rent
                            );
                            book.deleted[actor] = true;
                            book.check(&world);
                        }
                        terminal_check(&world, payouts, backing, false);
                        assert_eq!(
                            world.env.token_amount(vault) as u128,
                            book.face_discount() + backing
                        );
                        let admin = world.env.admin.insecure_clone();
                        let destination = create_ata_for_test(
                            &mut world.env.svm,
                            &world.env.payer,
                            admin.pubkey(),
                            world.env.mint,
                        );
                        let mint = world.env.mint;
                        let initial_rent =
                            world.env.svm.get_account(&admin.pubkey()).unwrap().lamports
                                + world.env.svm.get_account(&market).unwrap().lamports
                                + world.env.svm.get_account(&vault).unwrap().lamports;
                        let close = Instruction {
                            program_id: world.env.program_id,
                            data: ProgInstruction::CloseSlab {
                                authority_epoch: world.env.control_sequences(0).authority_epoch,
                            }
                            .encode(),
                            accounts: vec![
                                AccountMeta::new(admin.pubkey(), true),
                                AccountMeta::new(market, false),
                                AccountMeta::new(vault, false),
                                AccountMeta::new_readonly(world.env.vault_authority, false),
                                AccountMeta::new(destination, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                                AccountMeta::new(mint, false),
                            ],
                        };
                        world.env.svm.warp_to_slot(EXPIRY + u64::from(late));
                        peak = peak.max(land(
                            &mut world,
                            &[close.clone(), denied.clone()],
                            &[&admin],
                            &[],
                            Some(1),
                        ));
                        rollbacks += 1;
                        terminal_check(&world, payouts, backing, false);
                        peak = peak.max(land(
                            &mut world,
                            &[close.clone()],
                            &[&admin],
                            &[market],
                            None,
                        ));
                        terminal_calls += 1;
                        terminal_check(&world, payouts, backing, true);
                        assert_eq!(
                            world.env.market_state().1.source_credit[source_domain]
                                .fresh_reserved_backing_num,
                            BOUND_SCALE,
                            "unrelated expiry cannot release the fractional source atom"
                        );
                        world.env.svm.warp_to_slot(SOURCE_EXPIRY + u64::from(late));
                        for _ in 0..16 {
                            if world.env.svm.get_account(&market).unwrap().data.len()
                                == percolator_prog::constants::HEADER_LEN
                            {
                                break;
                            }
                            peak = peak.max(land(
                                &mut world,
                                &[close.clone(), denied.clone()],
                                &[&admin],
                                &[],
                                Some(1),
                            ));
                            rollbacks += 1;
                            terminal_check(&world, payouts, backing, true);
                            let before = world.frame();
                            peak = peak.max(land(
                                &mut world,
                                &[close.clone()],
                                &[&admin],
                                &[market, vault, mint, admin.pubkey()],
                                None,
                            ));
                            terminal_calls += 1;
                            assert_ne!(world.frame(), before, "terminal continuation advances");
                            if world.env.svm.get_account(&market).unwrap().data.len()
                                != percolator_prog::constants::HEADER_LEN
                            {
                                terminal_check(&world, payouts, backing, true);
                            }
                        }
                        assert_closed_market_tombstone(
                            &world.env.svm.get_account(&market).unwrap(),
                        );
                        let tombstone_rent = world.env.svm.get_account(&market).unwrap().lamports;
                        assert_eq!(
                            tombstone_rent,
                            world.env.svm.minimum_balance_for_rent_exemption(
                                percolator_prog::constants::HEADER_LEN
                            )
                        );
                        assert_eq!(
                            world.env.svm.get_account(&admin.pubkey()).unwrap().lamports,
                            initial_rent - tombstone_rent
                        );
                        assert!(world
                            .env
                            .svm
                            .get_account(&vault)
                            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                        assert_eq!(world.env.token_amount(destination), 0);
                        let supply = Mint::unpack(&world.env.svm.get_account(&mint).unwrap().data)
                            .unwrap()
                            .supply as u128;
                        assert_eq!(supply, payouts.into_iter().sum::<u128>());
                        assert_eq!(DEPOSITS.into_iter().sum::<u128>() - supply, book.face_discount() + backing,
                            "fractional source residue and expired principal have one terminal burn");
                        let wallets: [u128; 5] = std::array::from_fn(|actor| {
                            world.env.token_amount(world.actors[actor].token) as u128
                        });
                        assert_eq!(wallets, payouts);
                        if let Some(expected) = normalized {
                            assert_eq!(wallets, expected);
                        } else {
                            normalized = Some(wallets);
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!((worlds, receipts), (32, 32));
    assert_eq!(rollbacks, 2 * worlds + terminal_calls);
    println!("INV-039 mixed fractional retirement: {worlds} worlds, {rollbacks} exact rollbacks, {receipts} paid receipt retries, {waits} waiting rollbacks, {terminal_calls} terminal calls; peak CU={peak}");
}

pub(super) fn native_world(reverse: bool, params: V16CuMarketParams) -> AttributionWorld {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;

    let mut env = inv081_public_native_market_with_params(3, params);
    env.portfolio_account_len = state::portfolio_account_len_for_market_slots(3).unwrap();
    let mut actors = Vec::new();
    for deposit in DEPOSITS {
        let owner = Keypair::new();
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
        let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&owner.pubkey(), &token, deposit as u64),
                spl_token::instruction::sync_native(&spl_token::ID, &token).unwrap(),
            ],
            &[&owner],
        )
        .unwrap();
        env.send(
            env.deposit_ix(portfolio.pubkey(), deposit),
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
        actors.push(AttributionActor {
            owner,
            portfolio: portfolio.pubkey(),
            token,
        });
    }
    for asset in 0..3 {
        env.configure_auth_mark_for_asset_as_admin(asset, 0, params.initial_price);
    }
    env.configure_permissionless_resolve_with_cu(100, 5);
    let q = POS_SCALE as i128 * if reverse { -1 } else { 1 };
    AttributionWorld {
        env,
        actors,
        quantities: [q, -q, -2 * q, 2 * q],
        deposits: DEPOSITS,
    }
}

fn native_image(before: &Account, amount: u64, raw: u64) -> Account {
    // Expected image only: never installed into the VM.
    let mut expected = before.clone();
    let mut token = TokenAccount::unpack(&before.data).unwrap();
    let COption::Some(rent) = token.is_native else {
        panic!("expected native custody");
    };
    assert_eq!(token.mint, spl_token::native_mint::ID);
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports = rent + amount + raw;
    expected
}

#[test]
fn v16_program_mixed_role_native_retirement_separates_owner_debt_residue_and_surplus() {
    let (mut worlds, mut peak, mut waits, mut retirements, mut rollbacks) = (0, 0, 0, 0, 0);
    let mut receipt_retries = 0;
    for debt in [36_000, 216_000] {
        for reverse in [false, true] {
            for order in [[0, 2, 1, 3, 4], [2, 1, 0, 4, 3]] {
                for (donation, sync) in [(0, false), (19, false), (19, true)] {
                    let (mut world, mut book) = setup_with_quote(reverse, [1, 2], debt, true);
                    let admin = world.env.admin.insecure_clone();
                    let insurer = world.actors[3].owner.insecure_clone();
                    assert_ne!(admin.pubkey(), insurer.pubkey());
                    world
                        .env
                        .try_update_per_asset_authority_with_cu(
                            &admin,
                            Some(&insurer),
                            0,
                            processor::ASSET_AUTH_INSURANCE,
                            insurer.pubkey().to_bytes(),
                        )
                        .unwrap();
                    book.check(&world);
                    assert!(!book.booked && !book.charged && !book.debt_settled);
                    let mint_frame = world.env.svm.get_account(&world.env.mint);
                    let before = world.frame();
                    let cu = world.env.resolve();
                    assert_cu_within("native mixed resolve", cu, CUSTODY_CU_LIMIT);
                    peak = peak.max(cu);
                    for (key, account) in before {
                        if key != world.env.market {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    book.check(&world);
                    assert!(!book.booked && !book.charged && !book.debt_settled);
                    world.env.svm.warp_to_slot(15);
                    for round in 0..16 {
                        let before = world.frame();
                        for actor in order {
                            if !resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                waits += usize::from(book.close(&mut world, actor, &mut peak));
                            }
                        }
                        if world
                            .actors
                            .iter()
                            .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                        {
                            break;
                        }
                        assert_ne!(
                            world.frame(),
                            before,
                            "native close round {round} must progress"
                        );
                    }
                    assert!(book.booked && book.charged && book.debt_settled);
                    let payouts = book.payouts();
                    let market = world.env.market;
                    let vault = world.env.vault;
                    let residue = book.face_discount() as u64;
                    assert_eq!(residue, if debt == 36_000 { 4_000 } else { 20_000 });
                    if debt == 216_000 {
                        assert_eq!(book.peer_receipt_face(), DEPOSITS[1] + 1);
                    }
                    let denied_deletions: [Instruction; 5] =
                        std::array::from_fn(|actor| deletion(&world, actor, false));
                    for actor in order {
                        let a = &world.actors[actor];
                        assert!(resolved_portfolio_is_terminal(&world.env, a.portfolio));
                        assert_eq!(world.env.token_amount(a.token) as u128, payouts[actor]);
                        let owner = a.owner.insecure_clone();
                        let token = a.token;
                        let portfolio = a.portfolio;
                        let receipt = resolved_receipt(&world.env.portfolio_state(portfolio));
                        if receipt.present {
                            assert_eq!(actor, 2);
                            assert_eq!(
                                (receipt.terminal_positive_claim_face, receipt.paid_effective),
                                (book.peer_receipt_face(), book.peer_receipt_face())
                            );
                        }
                        let token_frame = world.env.svm.get_account(&token).unwrap();
                        let rent = token_frame.lamports - payouts[actor] as u64;
                        let wallet_before = world.env.svm.get_account(&owner.pubkey()).unwrap();
                        assert_eq!(
                            wallet_before.lamports,
                            1_000_000_000 - DEPOSITS[actor] as u64
                        );
                        let redeem = spl_token::instruction::close_account(
                            &spl_token::ID,
                            &token,
                            &owner.pubkey(),
                            &owner.pubkey(),
                            &[],
                        )
                        .unwrap();
                        let denied = denied_deletions[(actor + 1) % 5].clone();
                        peak = peak.max(land(
                            &mut world,
                            &[redeem.clone(), denied],
                            &[&owner],
                            &[],
                            Some(1),
                        ));
                        rollbacks += 1;
                        peak = peak.max(land(
                            &mut world,
                            &[redeem],
                            &[&owner],
                            &[token, owner.pubkey()],
                            None,
                        ));
                        assert!(world
                            .env
                            .svm
                            .get_account(&token)
                            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                        let mut wallet_after = wallet_before;
                        wallet_after.lamports += rent + payouts[actor] as u64;
                        assert_eq!(
                            world.env.svm.get_account(&owner.pubkey()),
                            Some(wallet_after)
                        );
                        let recreated = create_ata_for_test(
                            &mut world.env.svm,
                            &world.env.payer,
                            owner.pubkey(),
                            world.env.mint,
                        );
                        assert_eq!(recreated, token);
                        assert_eq!(
                            world.env.svm.get_account(&token),
                            Some(native_image(&token_frame, 0, 0))
                        );
                        if receipt.present {
                            let retry = payout(&world, actor, true);
                            peak = peak.max(land(&mut world, &[retry], &[], &[], None));
                            receipt_retries += 1;
                            assert_eq!(
                                world.env.token_amount(token),
                                0,
                                "redeeming custody cannot renew a paid receipt"
                            );
                        }
                        let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                        let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                        let delete = deletion(&world, actor, true);
                        peak = peak.max(land(
                            &mut world,
                            &[delete],
                            &[&owner],
                            &[market, portfolio],
                            None,
                        ));
                        assert_eq!(
                            world.env.svm.get_account(&market).unwrap().lamports,
                            market_rent + rent
                        );
                        book.deleted[actor] = true;
                    }
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.materialized_portfolio_count,
                            group.insurance
                        ),
                        (0, 0, 0, 0)
                    );
                    assert_eq!(group.vault, residue as u128);
                    assert_market_stock_census(
                        "native mixed completion",
                        &group,
                        &world.env.svm.get_account(&market).unwrap().data,
                        &[],
                        residue as u128,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census("native mixed completion", &group, &[])
                        .unwrap();
                    assert_eq!(
                        payouts.iter().sum::<u128>() + residue as u128,
                        DEPOSITS.iter().sum::<u128>()
                    );
                    let paid_wallets: Vec<_> = world
                        .actors
                        .iter()
                        .map(|a| world.env.svm.get_account(&a.owner.pubkey()))
                        .collect();
                    let paid_tokens: Vec<_> = world
                        .actors
                        .iter()
                        .map(|a| world.env.svm.get_account(&a.token).unwrap())
                        .collect();
                    let destination = create_ata_for_test(
                        &mut world.env.svm,
                        &world.env.payer,
                        admin.pubkey(),
                        world.env.mint,
                    );
                    let destination_frame = world.env.svm.get_account(&destination).unwrap();
                    let residue_token = world.actors[3].token;
                    let close = Instruction {
                        program_id: world.env.program_id,
                        data: ProgInstruction::CloseSlab {
                            authority_epoch: world.env.control_sequences(0).authority_epoch,
                        }
                        .encode(),
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(market, false),
                            AccountMeta::new(vault, false),
                            AccountMeta::new_readonly(world.env.vault_authority, false),
                            AccountMeta::new(destination, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(world.env.mint, false),
                            AccountMeta::new(residue_token, false),
                        ],
                    };
                    let vault_frame = world.env.svm.get_account(&vault).unwrap();
                    let mut donated = vec![system_instruction::transfer(
                        &admin.pubkey(),
                        &vault,
                        donation,
                    )];
                    if sync {
                        donated.push(
                            spl_token::instruction::sync_native(&spl_token::ID, &vault).unwrap(),
                        );
                    }
                    peak = peak.max(land(
                        &mut world,
                        &donated,
                        &[&admin],
                        &[vault, admin.pubkey()],
                        None,
                    ));
                    let wrapped = if sync { donation } else { 0 };
                    assert_eq!(
                        world.env.svm.get_account(&vault),
                        Some(native_image(
                            &vault_frame,
                            residue + wrapped,
                            donation - wrapped
                        ))
                    );
                    let admin_before = world.env.svm.get_account(&admin.pubkey()).unwrap();
                    let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                    let vault_rent = vault_frame.lamports - residue;
                    world.env.svm.warp_to_slot(SOURCE_EXPIRY);
                    let denied = denied_deletions[0].clone();
                    for _ in 0..16 {
                        if world.env.svm.get_account(&market).unwrap().data.len()
                            == percolator_prog::constants::HEADER_LEN
                        {
                            break;
                        }
                        peak = peak.max(land(
                            &mut world,
                            &[close.clone(), denied.clone()],
                            &[&admin],
                            &[],
                            Some(1),
                        ));
                        rollbacks += 1;
                        let before = world.frame();
                        peak = peak.max(land(
                            &mut world,
                            &[close.clone()],
                            &[&admin],
                            &[market, vault, destination, residue_token, admin.pubkey()],
                            None,
                        ));
                        retirements += 1;
                        assert_ne!(world.frame(), before, "native retirement must progress");
                        if world.env.svm.get_account(&market).unwrap().data.len()
                            != percolator_prog::constants::HEADER_LEN
                        {
                            let group = world.env.market_state().1;
                            assert_eq!(
                                (
                                    group.vault,
                                    group.c_tot,
                                    group.pnl_pos_tot,
                                    group.materialized_portfolio_count
                                ),
                                (residue as u128, 0, 0, 0)
                            );
                            assert_market_stock_census(
                                "native mixed retirement",
                                &group,
                                &world.env.svm.get_account(&market).unwrap().data,
                                &[],
                                // The public donation is custody surplus, not booked stock.
                                (world.env.token_amount(vault) - wrapped) as u128,
                            )
                            .unwrap();
                            assert_eq!(
                                world.env.svm.get_account(&vault),
                                Some(native_image(
                                    &vault_frame,
                                    residue + wrapped,
                                    donation - wrapped
                                ))
                            );
                            assert_reservation_encumbrance_census(
                                "native mixed retirement",
                                &group,
                                &[],
                            )
                            .unwrap();
                            assert_eq!(
                                world.env.svm.get_account(&residue_token),
                                Some(paid_tokens[3].clone())
                            );
                            assert_eq!(
                                world.env.svm.get_account(&destination),
                                Some(destination_frame.clone())
                            );
                        }
                    }
                    let tombstone = world.env.svm.get_account(&market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(
                        tombstone.lamports,
                        world.env.svm.minimum_balance_for_rent_exemption(
                            percolator_prog::constants::HEADER_LEN
                        )
                    );
                    assert!(world
                        .env
                        .svm
                        .get_account(&vault)
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    let mut expected_admin = admin_before;
                    expected_admin.lamports +=
                        market_rent + vault_rent + donation - wrapped - tombstone.lamports;
                    assert_eq!(
                        world.env.svm.get_account(&admin.pubkey()),
                        Some(expected_admin)
                    );
                    assert_eq!(
                        world.env.svm.get_account(&destination),
                        Some(native_image(&destination_frame, wrapped, 0))
                    );
                    for (i, actor) in world.actors.iter().enumerate() {
                        assert_eq!(
                            world.env.svm.get_account(&actor.owner.pubkey()),
                            paid_wallets[i]
                        );
                        assert_eq!(
                            world.env.svm.get_account(&actor.token),
                            Some(native_image(
                                &paid_tokens[i],
                                if i == 3 { residue } else { 0 },
                                0
                            ))
                        );
                        assert!(world
                            .env
                            .svm
                            .get_account(&actor.portfolio)
                            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    }
                    assert_eq!(world.env.svm.get_account(&world.env.mint), mint_frame);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 24);
    assert_eq!(receipt_retries, worlds);
    assert!(waits > 0);
    assert_eq!(rollbacks, 5 * worlds + retirements);
    assert_cu_within("native mixed-role campaign peak", peak, CUSTODY_CU_LIMIT);
    println!("INV-039 native mixed retirement: {worlds} worlds, {waits} waiting rollbacks, {rollbacks} successful-prefix rollbacks, {receipt_retries} paid receipt retries, {retirements} retirement calls; peak {peak} CU");
}
