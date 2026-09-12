//! Rows 417/419: backing expiry does not settle a pending owner's opposing debt.
//! Public funded construction, exact/late normalization, claimant order, staged
//! settlement rollback and debtor deletion precede an input-derived terminal exit.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const BACKING: u128 = 97;
const EXPIRY: u64 = 25;
const DEBTS: [u128; 2] = [1, 2 * 19_999];
const SUPPLY: u128 = 930_777;
const PAYOUTS: [u128; 5] = [200_001, 179_999, 339_998, 210_002, 680];
const DEBT_BACKING_HORIZON: u64 = 6_480_000;
const LIMIT: u64 = 400_000;

struct Model {
    basis: [i128; 4],
    pending: [bool; 4],
    domain: usize,
    expired: bool,
    deleted: Option<usize>,
    settlement_slot: u64,
    paid: [bool; 2],
}

impl Model {
    fn check(&self, world: &AttributionWorld, provider: Pubkey) {
        world.check_with_deleted_debtor(self.basis, self.pending, self.deleted);
        let env = &world.env;
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.resolved_slot, 20);
        assert_eq!(
            group.materialized_portfolio_count,
            5 - u64::from(self.deleted.is_some())
        );
        assert_eq!(group.insurance, 0);
        assert_eq!(env.token_amount(provider), 0);
        let fresh = if self.basis[1] == 0 {
            if self.paid[0] {
                0
            } else {
                DEBTS[0] * BOUND_SCALE
            }
        } else if self.expired {
            0
        } else {
            BACKING * BOUND_SCALE
        };
        let bucket = group.source_backing_buckets[self.domain];
        assert_eq!(group.config.h_max, DEBT_BACKING_HORIZON);
        assert_eq!(
            bucket.expiry_slot,
            if self.basis[1] == 0 {
                self.settlement_slot + DEBT_BACKING_HORIZON
            } else {
                EXPIRY
            }
        );
        assert_eq!(
            bucket.status,
            if fresh == 0 {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(bucket.fresh_unliened_backing_num, fresh);
        assert_eq!(
            group.source_credit[self.domain].fresh_reserved_backing_num,
            fresh
        );
        for (domain, source) in group.source_credit.iter().enumerate() {
            let spent = (0..2)
                .filter(|pair| domain == self.domain + 2 * pair && self.paid[*pair])
                .map(|pair| DEBTS[pair] * BOUND_SCALE)
                .sum::<u128>();
            assert_eq!(source.spent_backing_num, spent);
            assert_eq!(source.provider_receivable_num, spent);
            assert_eq!(
                group.source_backing_buckets[domain].consumed_liened_backing_num,
                spent
            );
        }
        assert!(group.insurance_domain_spent.iter().all(|spent| *spent == 0));
        for (actor, a) in world.actors.iter().enumerate() {
            let expected = match actor {
                0 | 2 => ATTRIBUTION_DEPOSITS[actor] + DEBTS[actor / 2],
                1 | 3 if self.basis[actor] == 0 => PAYOUTS[actor],
                4 => ATTRIBUTION_DEPOSITS[4] - BACKING,
                _ => ATTRIBUTION_DEPOSITS[actor],
            };
            let paid = u128::from(env.token_amount(a.token));
            if Some(actor) == self.deleted {
                assert_eq!(paid, expected);
                continue;
            }
            let account = env.portfolio_state(a.portfolio);
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert!(matches!(actor, 0 | 2));
                assert_eq!(receipt.terminal_positive_claim_face, DEBTS[actor / 2]);
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get() + due as i128 + paid as i128,
                expected as i128,
                "owner {actor}: expiry cannot pay or forgive the original debt"
            );
            if matches!(actor, 0 | 2) && (self.basis[1] != 0 || self.basis[3] != 0) {
                assert_eq!(paid, 0);
                assert_eq!(account.pnl.get(), DEBTS[actor / 2] as i128);
                assert!(!receipt.present);
                assert!(!group.payout_snapshot_captured);
            }
            let token_account = env.svm.get_account(&a.token).unwrap();
            assert_eq!(token_account.owner, spl_token::ID);
            let token = TokenAccount::unpack(&token_account.data).unwrap();
            assert_eq!((token.owner, token.mint), (a.owner.pubkey(), env.mint));
            assert_eq!(
                (token.delegate, token.close_authority, token.is_native),
                (COption::None, COption::None, COption::None)
            );
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(u128::from(mint.supply), SUPPLY);
        let portfolios: Vec<_> = world
            .actors
            .iter()
            .enumerate()
            .filter(|(actor, _)| Some(*actor) != self.deleted)
            .map(|(_, actor)| env.portfolio_state(actor.portfolio))
            .collect();
        crate::support::fuzz_model::assert_market_stock_census(
            "pending expiry prefix",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &portfolios,
            u128::from(env.token_amount(env.vault)),
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "pending expiry prefix",
            &group,
            &portfolios,
        )
        .unwrap();
    }
}

fn payout(world: &AttributionWorld, actor: usize) -> Instruction {
    let env = &world.env;
    let a = &world.actors[actor];
    Instruction {
        program_id: env.program_id,
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
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

fn land(
    world: &mut AttributionWorld,
    provider: Pubkey,
    ixs: &[Instruction],
    signers: &[&Keypair],
    allowed: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
    successes: (usize, usize),
) -> u64 {
    world.env.svm.expire_blockhash();
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(ixs);
    let mut signing = vec![&world.env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&world.env.payer.pubkey()),
        &signing,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.push(provider);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    let result = world.env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = rejection {
        assert!(allowed.is_empty());
        let failure = result.expect_err("pending debt still prevents the claimant's payout");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, error)
        );
        failure.meta
    } else {
        result.expect("public expiry/debt continuation")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if !allowed.contains(&key) {
            assert_eq!(
                world.env.svm.get_account(&key),
                expected,
                "complete Account frame {key}"
            );
        }
    }
    for (program, count) in [
        (world.env.program_id, successes.0),
        (spl_token::ID, successes.1),
    ] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count,
            "intended wrapper/SPL prefixes must execute"
        );
    }
    assert_cu_within("pending backing expiry", meta.compute_units_consumed, LIMIT);
    meta.compute_units_consumed
}

fn setup(reverse: bool, settlement_slot: u64) -> (AttributionWorld, Model, Pubkey) {
    assert_eq!(
        PAYOUTS,
        std::array::from_fn(|actor| match actor {
            0 | 2 => ATTRIBUTION_DEPOSITS[actor] + DEBTS[actor / 2],
            1 | 3 => ATTRIBUTION_DEPOSITS[actor] - DEBTS[actor / 2],
            _ => ATTRIBUTION_DEPOSITS[actor] - BACKING,
        })
    );
    assert_eq!(PAYOUTS.iter().sum::<u128>() + BACKING, SUPPLY);
    let mut world = AttributionWorld::new(reverse);
    let env = &mut world.env;
    let admin = env.admin.insecure_clone();
    let provider = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    let donor = &world.actors[4];
    env.send(
        env.withdraw_ix(donor.portfolio, BACKING),
        vec![
            AccountMeta::new(donor.owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(donor.portfolio, false),
            AccountMeta::new(donor.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&donor.owner],
    )
    .unwrap();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &donor.token,
            &provider,
            &donor.owner.pubkey(),
            &[],
            BACKING as u64,
        )
        .unwrap(),
        &[&donor.owner],
    )
    .unwrap();
    let domain = if reverse { 2 } else { 3 };
    env.top_up_backing_bucket_from_admin_token_with_cu(provider, domain as u16, BACKING, EXPIRY);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&admin],
    )
    .unwrap();
    let sign = if reverse { -1 } else { 1 };
    for (pair, lots) in [1, 2].into_iter().enumerate() {
        let q = sign * lots * POS_SCALE as i128;
        world.quantities[2 * pair] = q;
        world.quantities[2 * pair + 1] = -q;
        env.trade_asset_with_cu(
            (pair + 1) as u16,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            q,
            1_000_000,
            0,
        );
    }
    env.svm.warp_to_slot(20);
    for (pair, movement) in [1, 19_999].into_iter().enumerate() {
        env.push_auth_mark_for_asset_as_admin(
            (pair + 1) as u16,
            20,
            (1_000_000 + sign * movement) as u64,
        );
    }
    env.crank(
        world.actors[4].portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations_for_assets(&[1, 2]),
        },
    );
    for pair in 0..2 {
        world.env.crank(
            world.actors[2 * pair].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations((pair + 1) as u16),
            },
        );
        world.env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            (pair + 1) as u16,
            20,
            0,
        );
        world.forfeit(2 * pair);
    }
    let model = Model {
        basis: [0, world.quantities[1], 0, world.quantities[3]],
        pending: [true, false, true, false],
        domain,
        expired: false,
        deleted: None,
        settlement_slot,
        paid: [false; 2],
    };
    world.check(model.basis, model.pending);
    let before = world.frame();
    world.env.resolve();
    for (key, account) in before {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    model.check(&world, provider);
    (world, model, provider)
}

#[test]
fn v16_program_pending_losses_survive_late_backing_expiry_and_claimant_close_order() {
    let mut peak = 0;
    for reverse in [false, true] {
        for slot in [EXPIRY, EXPIRY + 6] {
            for normalizer in [0, 1] {
                for order in [[0, 2], [2, 0]] {
                    let (mut world, mut model, provider) = setup(reverse, slot);
                    let retained = [0, 1, 2, 3, 4].map(|actor| payout(&world, actor));
                    let portfolios: [Pubkey; 5] =
                        std::array::from_fn(|actor| world.actors[actor].portfolio);
                    let tokens: [Pubkey; 5] =
                        std::array::from_fn(|actor| world.actors[actor].token);
                    world.env.svm.warp_to_slot(slot);
                    model.check(&world, provider);
                    // Expiry, a real debtor SPL payout and the other holder's detach all
                    // execute before that holder's waiting retry rejects the transaction.
                    peak = peak.max(land(
                        &mut world,
                        provider,
                        &[
                            retained[normalizer].clone(),
                            retained[1].clone(),
                            retained[2].clone(),
                            retained[2].clone(),
                        ],
                        &[],
                        &[],
                        Some((
                            5,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                        (3, 1),
                    ));
                    model.check(&world, provider);
                    let market = world.env.market;
                    let vault = world.env.vault;
                    peak = peak.max(land(
                        &mut world,
                        provider,
                        &[retained[normalizer].clone()],
                        &[],
                        &[market, portfolios[normalizer]],
                        None,
                        (1, 0),
                    ));
                    model.expired = true;
                    model.check(&world, provider);
                    peak = peak.max(land(
                        &mut world,
                        provider,
                        &[retained[1].clone()],
                        &[],
                        &[market, vault, portfolios[1], tokens[1]],
                        None,
                        (1, 1),
                    ));
                    model.basis[1] = 0;
                    model.check(&world, provider);
                    let owner = world.actors[1].owner.insecure_clone();
                    let debtor = world.actors[1].portfolio;
                    let debtor_rent = world.env.svm.get_account(&debtor).unwrap().lamports;
                    let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                    let close_debtor = Instruction {
                        program_id: world.env.program_id,
                        data: world.env.close_portfolio_ix(debtor).encode(),
                        accounts: vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(market, false),
                            AccountMeta::new(debtor, false),
                        ],
                    };
                    peak = peak.max(land(
                        &mut world,
                        provider,
                        &[close_debtor],
                        &[&owner],
                        &[market, debtor],
                        None,
                        (1, 0),
                    ));
                    assert_eq!(
                        world.env.svm.get_account(&market).unwrap().lamports,
                        market_rent + debtor_rent
                    );
                    model.deleted = Some(1);
                    model.check(&world, provider);
                    for actor in order {
                        peak = peak.max(land(
                            &mut world,
                            provider,
                            &[retained[actor].clone()],
                            &[],
                            &[market, portfolios[actor]],
                            None,
                            (1, 0),
                        ));
                        model.pending[actor] = false;
                        model.check(&world, provider);
                    }
                    peak = peak.max(land(
                        &mut world,
                        provider,
                        &[retained[order[0]].clone()],
                        &[],
                        &[],
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                        (0, 0),
                    ));
                    model.check(&world, provider);
                    for actor in [3, order[0], order[1], 4] {
                        peak = peak.max(land(
                            &mut world,
                            provider,
                            &[retained[actor].clone()],
                            &[],
                            &[market, vault, portfolios[actor], tokens[actor]],
                            None,
                            (1, 1),
                        ));
                        if actor == 3 {
                            model.basis[3] = 0;
                        }
                        if matches!(actor, 0 | 2) {
                            model.paid[actor / 2] = true;
                        }
                        model.check(&world, provider);
                        assert_eq!(
                            u128::from(world.env.token_amount(world.actors[actor].token)),
                            PAYOUTS[actor]
                        );
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                    }
                    assert_eq!(model.basis, [0; 4]);
                    assert_eq!(model.pending, [false; 4]);
                    assert_eq!(world.env.market_state().1.vault, BACKING);
                    for actor in [order[0], 3, order[1], 4] {
                        let owner = world.actors[actor].owner.insecure_clone();
                        let portfolio = world.actors[actor].portfolio;
                        let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                        let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                        let close = Instruction {
                            program_id: world.env.program_id,
                            data: world.env.close_portfolio_ix(portfolio).encode(),
                            accounts: vec![
                                AccountMeta::new(owner.pubkey(), true),
                                AccountMeta::new(market, false),
                                AccountMeta::new(portfolio, false),
                            ],
                        };
                        peak = peak.max(land(
                            &mut world,
                            provider,
                            &[close],
                            &[&owner],
                            &[market, portfolio],
                            None,
                            (1, 0),
                        ));
                        assert_eq!(
                            world.env.svm.get_account(&market).unwrap().lamports,
                            market_rent + rent
                        );
                        assert!(world
                            .env
                            .svm
                            .get_account(&portfolio)
                            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    }
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.materialized_portfolio_count,
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.insurance,
                            group.vault
                        ),
                        (0, 0, 0, 0, BACKING)
                    );
                    crate::support::fuzz_model::assert_market_stock_census(
                        "pending expiry terminal",
                        &group,
                        &world.env.svm.get_account(&market).unwrap().data,
                        &[],
                        BACKING,
                    )
                    .unwrap();
                    crate::support::fuzz_model::assert_reservation_encumbrance_census(
                        "pending expiry terminal",
                        &group,
                        &[],
                    )
                    .unwrap();
                    let admin = world.env.admin.insecure_clone();
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
                            AccountMeta::new(provider, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(world.env.mint, false),
                        ],
                    };
                    let rent = world.env.svm.get_account(&market).unwrap().lamports
                        + world.env.svm.get_account(&vault).unwrap().lamports;
                    let mut expected_admin = world.env.svm.get_account(&admin.pubkey()).unwrap();
                    let mint = world.env.mint;
                    peak = peak.max(land(
                        &mut world,
                        provider,
                        &[close],
                        &[&admin],
                        &[market, vault, mint, admin.pubkey()],
                        None,
                        (1, 2),
                    ));
                    let tombstone = world.env.svm.get_account(&market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(
                        tombstone.lamports,
                        world.env.svm.minimum_balance_for_rent_exemption(
                            percolator_prog::constants::HEADER_LEN
                        )
                    );
                    expected_admin.lamports += rent - tombstone.lamports;
                    assert_eq!(
                        world.env.svm.get_account(&admin.pubkey()),
                        Some(expected_admin)
                    );
                    assert!(world
                        .env
                        .svm
                        .get_account(&vault)
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    assert_eq!(
                        u128::from(
                            Mint::unpack(&world.env.svm.get_account(&mint).unwrap().data)
                                .unwrap()
                                .supply
                        ),
                        SUPPLY - BACKING
                    );
                    assert_eq!(
                        world
                            .actors
                            .iter()
                            .map(|a| u128::from(world.env.token_amount(a.token)))
                            .collect::<Vec<_>>(),
                        PAYOUTS
                    );
                    assert_eq!(world.env.token_amount(provider), 0);
                }
            }
        }
    }
    eprintln!("pending backing expiry: 16 worlds, 32 exact rollbacks, 80 owner payouts, 80 portfolio deletions, 16 slab closes, peak={peak} CU");
}
