//! INV-039/024: claimant and reserve roles retain separate entitlements from
//! pending bankruptcy through debt booking, deletion, expiry and recredit.
//! Scope J's same-portfolio creditor/debtor composition is a separate control.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const INSURANCE: [u128; 2] = [7_500, 6_000];
const PRINCIPAL_PREFIX: u128 = 31;
const EXPIRY: u64 = 25;

fn land(
    world: &mut AttributionWorld,
    instructions: &[Instruction],
    signers: &[&Keypair],
    changed: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    let env = &mut world.env;
    env.svm.expire_blockhash();
    let mut batch = vec![heap_ix(), cu_ix()];
    batch.extend_from_slice(instructions);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &batch,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
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
    let result = world.env.svm.send_transaction(tx);
    let rejected = rejection.is_some();
    let meta = if let Some((index, error)) = rejection {
        let failure = result.expect_err("specified terminal prerequisite");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", world.env.program_id))
                .count(),
            usize::from(index - 2)
        );
        failure.meta
    } else {
        result.expect("public obligation/reserve continuation")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !changed.contains(&key) {
            assert_eq!(
                world.env.svm.get_account(&key),
                expected,
                "complete Account {key}"
            );
        }
    }
    assert_cu_within(
        "pending reserve role transaction",
        meta.compute_units_consumed,
        600_000,
    );
    meta.compute_units_consumed
}

fn reserve(
    world: &AttributionWorld,
    actor: usize,
    domain: usize,
    principal: bool,
    amount: u128,
) -> Instruction {
    let env = &world.env;
    let data = if principal {
        ProgInstruction::WithdrawBackingBucket {
            domain: domain as u16,
            market_id: env.asset_market_id(1),
            authority_epoch: env.control_sequences(1).authority_epoch,
            amount,
        }
    } else {
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 1,
            market_id: env.asset_market_id(1),
            authority_epoch: env.control_sequences(1).authority_epoch,
            amount,
        }
    };
    Instruction {
        program_id: env.program_id,
        data: data.encode(),
        accounts: vec![
            AccountMeta::new_readonly(world.actors[actor].owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(world.actors[actor].token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    }
}

fn fund_backing(world: &mut AttributionWorld, provider: usize, domain: usize, amount: u128) {
    let env = &mut world.env;
    let donor = &world.actors[4];
    let holder = &world.actors[provider];
    env.send(
        env.withdraw_ix(donor.portfolio, amount),
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
            &holder.token,
            &donor.owner.pubkey(),
            &[],
            amount as u64,
        )
        .unwrap(),
        &[&donor.owner],
    )
    .unwrap();
    env.send(
        ProgInstruction::TopUpBackingBucket {
            domain: domain as u16,
            market_id: env.asset_market_id(1),
            authority_epoch: env.control_sequences(1).authority_epoch,
            intent_id: next_control_sequence(env.control_sequences(1).backing_top_up),
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount,
            expiry_slot: EXPIRY,
        },
        vec![
            AccountMeta::new(holder.owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(holder.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&holder.owner],
    )
    .unwrap();
}

struct ReserveBook {
    users: [u128; 5],
    provider: usize,
    beneficiary: usize,
    domain: usize,
    backing: u128,
    principal_paid: u128,
    insurance_paid: u128,
    recredited: bool,
    expired: bool,
    deleted: [bool; 5],
}

impl ReserveBook {
    fn recovered(&self) -> u128 {
        // Only the selected asset has free residue; both debts have been paid.
        (self.backing - PRINCIPAL_PREFIX)
            .min(INSURANCE[0])
            .min(ATTRIBUTION_DEPOSITS[1])
    }

    fn wallet_matches(actual: [u128; 5], by_class: [[u128; 3]; 5]) -> bool {
        actual == by_class.map(|classes| classes.into_iter().sum::<u128>())
    }

    fn class_matches(actual: [u128; 2], expected: [u128; 2]) -> bool {
        actual == expected
    }

    fn check(&self, world: &AttributionWorld) -> [u128; 5] {
        let env = &world.env;
        let group = env.market_state().1;
        let mut paid = self.users.map(|user| [user, 0, 0]);
        paid[self.provider][1] = self.principal_paid;
        paid[self.beneficiary][2] = self.insurance_paid;
        let actual: [u128; 5] =
            std::array::from_fn(|actor| env.token_amount(world.actors[actor].token) as u128);
        assert!(
            Self::wallet_matches(actual, paid),
            "user/provider/beneficiary payouts: {actual:?}, book={paid:?}"
        );
        let mut wrong_owner = actual;
        wrong_owner[self.beneficiary] -= 1;
        wrong_owner[4] += 1;
        assert_eq!(
            wrong_owner.iter().sum::<u128>(),
            actual.iter().sum::<u128>()
        );
        assert!(!Self::wallet_matches(wrong_owner, paid));
        // Even when both reserves share an ATA, their remaining classes differ.
        let recovery = if self.recredited { self.recovered() } else { 0 };
        assert_eq!(group.insurance, recovery - self.insurance_paid);
        for pair in 0..2 {
            let domain = 2 * (pair + 1) + usize::from(world.quantities[2 * pair] < 0);
            assert_eq!(
                group.insurance_domain_budget[domain],
                INSURANCE[pair] - if pair == 0 { self.insurance_paid } else { 0 }
            );
            assert_eq!(
                group.insurance_domain_spent[domain],
                INSURANCE[pair] - if pair == 0 { recovery } else { 0 }
            );
        }
        let bucket = group.source_backing_buckets[self.domain];
        let principal = if self.expired {
            0
        } else {
            self.backing - self.principal_paid
        };
        let classes = [
            bucket.fresh_unliened_backing_num / BOUND_SCALE,
            group.insurance,
        ];
        let expected_classes = [principal, recovery - self.insurance_paid];
        assert!(
            Self::class_matches(classes, expected_classes),
            "reserve classes: {classes:?}, expected={expected_classes:?}, expired={}",
            self.expired
        );
        if let Some(from) = classes.iter().position(|amount| *amount != 0) {
            let mut wrong_class = classes;
            wrong_class[from] -= 1;
            wrong_class[1 - from] += 1;
            assert_eq!(
                wrong_class.iter().sum::<u128>(),
                classes.iter().sum::<u128>()
            );
            assert!(!Self::class_matches(wrong_class, expected_classes));
        }
        assert_eq!(bucket.fresh_unliened_backing_num, principal * BOUND_SCALE);
        assert_eq!(bucket.utilization_fee_earnings, 0);
        assert_eq!(bucket.valid_liened_backing_num, 0);
        assert_eq!(bucket.consumed_liened_backing_num, 0);
        assert_eq!(bucket.expiry_slot, EXPIRY);
        assert_eq!(
            bucket.status,
            if self.expired {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        assert_eq!(
            group.source_credit[self.domain].fresh_reserved_backing_num,
            principal * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[self.domain ^ 1].provider_receivable_num,
            ATTRIBUTION_DEPOSITS[1] * BOUND_SCALE
        );
        assert_eq!(
            group.vault,
            self.backing - self.principal_paid - self.insurance_paid
        );
        assert_eq!(group.vault, env.token_amount(env.vault) as u128);
        assert_eq!(group.c_tot, 0);
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(
            group.materialized_portfolio_count,
            self.deleted.iter().filter(|deleted| !**deleted).count() as u64
        );
        let mut portfolios = Vec::new();
        for (actor, owner) in world.actors.iter().enumerate() {
            let token_account = env.svm.get_account(&owner.token).unwrap();
            let token = TokenAccount::unpack(&token_account.data).unwrap();
            assert_eq!(token_account.owner, spl_token::ID);
            assert_eq!((token.owner, token.mint), (owner.owner.pubkey(), env.mint));
            if self.deleted[actor] {
                assert!(env
                    .svm
                    .get_account(&owner.portfolio)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            } else {
                assert!(resolved_portfolio_is_terminal(env, owner.portfolio));
                portfolios.push(env.portfolio_state(owner.portfolio));
            }
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply as u128, world.deposits.iter().sum::<u128>());
        assert_eq!(
            actual.iter().sum::<u128>() + group.vault,
            mint.supply as u128
        );
        assert_market_stock_census(
            "pending reserve roles",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &portfolios,
            group.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("pending reserve roles", &group, &portfolios)
            .unwrap();
        actual
    }
}

#[test]
fn v16_program_pending_claimant_reserve_roles_preserve_attribution_through_close_and_recredit() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for backing in [338, 7_531] {
        for reverse in [false, true] {
            for (provider, beneficiary) in [(0, 0), (2, 0), (0, 2)] {
                let mut normalized = None;
                for schedule in 0..2 {
                    let mut deposits = ATTRIBUTION_DEPOSITS;
                    deposits[4] = DONOR_DEPOSIT + backing;
                    let mut world = setup_with_deposits(reverse, &mut peak, deposits);
                    let source = fund(&mut world, INSURANCE);
                    let domain = 2 + usize::from(world.quantities[0] < 0);
                    let before_roles: Vec<_> = world
                        .actors
                        .iter()
                        .map(|a| world.env.svm.get_account(&a.portfolio))
                        .collect();
                    let admin = world.env.admin.insecure_clone();
                    for (kind, actor) in [
                        (processor::ASSET_AUTH_BACKING_BUCKET, provider),
                        (processor::ASSET_AUTH_INSURANCE, beneficiary),
                    ] {
                        let incoming = world.actors[actor].owner.insecure_clone();
                        world
                            .env
                            .try_update_per_asset_authority_with_cu(
                                &admin,
                                Some(&incoming),
                                1,
                                kind,
                                incoming.pubkey().to_bytes(),
                            )
                            .unwrap();
                    }
                    for (a, before) in world.actors.iter().zip(before_roles) {
                        assert_eq!(
                            world.env.svm.get_account(&a.portfolio),
                            before,
                            "reserve roles cannot settle pending obligations"
                        );
                    }
                    fund_backing(&mut world, provider, domain, backing);
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &world.env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                    let mut debt = InsuredDebtBook {
                        initial: std::array::from_fn(|pair| {
                            close_progress(
                                &world
                                    .env
                                    .portfolio_state(world.actors[2 * pair + 1].portfolio),
                            )
                        }),
                        insurance: INSURANCE,
                        booked: [false; 2],
                        released: [false; 2],
                        source,
                    };
                    debt.check(&world);
                    world.env.resolve();
                    world.env.svm.warp_to_slot(10);
                    debt.check(&world);
                    let early = schedule;
                    let late = 1 - early;
                    let mut denied = close_instruction(&world, 4);
                    denied.accounts[0].is_signer = false;
                    for pair in [early, late] {
                        debt.step(&mut world, 2 * pair, &mut peak);
                        let booking = terminal_instruction(&world, 2 * pair + 1);
                        peak = peak.max(land(
                            &mut world,
                            &[booking, denied.clone()],
                            &[],
                            &[],
                            Some((3, PercolatorError::ExpectedSigner)),
                        ));
                        rollbacks += 1;
                        debt.check(&world);
                        debt.step(&mut world, 2 * pair + 1, &mut peak);
                        debt.step(&mut world, 2 * pair, &mut peak);
                    }
                    let order = if schedule == 0 {
                        [0, 2, 4, 1, 3]
                    } else {
                        [2, 0, 4, 3, 1]
                    };
                    for actor in order {
                        for _ in 0..4 {
                            if resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                break;
                            }
                            let payment = terminal_instruction(&world, actor);
                            peak = peak.max(land(
                                &mut world,
                                &[payment, denied.clone()],
                                &[],
                                &[],
                                Some((3, PercolatorError::ExpectedSigner)),
                            ));
                            rollbacks += 1;
                            debt.check(&world);
                            debt.step(&mut world, actor, &mut peak);
                        }
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                    }
                    let mut book = ReserveBook {
                        users: std::array::from_fn(|actor| debt.payout(actor)),
                        provider,
                        beneficiary,
                        domain,
                        backing,
                        principal_paid: 0,
                        insurance_paid: 0,
                        recredited: false,
                        expired: false,
                        deleted: [false; 5],
                    };
                    book.check(&world);
                    let market = world.env.market;
                    let vault = world.env.vault;
                    let provider_token = world.actors[provider].token;
                    let beneficiary_token = world.actors[beneficiary].token;
                    let principal = reserve(&world, provider, domain, true, PRINCIPAL_PREFIX);
                    let insurer = reserve(&world, beneficiary, domain, false, book.recovered());
                    let deletions = if schedule == 0 {
                        [1, 3, 2, 4, 0]
                    } else {
                        [0, 2, 1, 4, 3]
                    };
                    for actor in deletions {
                        let owner = world.actors[actor].owner.insecure_clone();
                        let portfolio = world.actors[actor].portfolio;
                        let ix = close_instruction(&world, actor);
                        let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                        let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                        if book.deleted.iter().filter(|deleted| !**deleted).count() == 1 {
                            peak = peak.max(land(
                                &mut world,
                                &[ix.clone(), principal.clone(), insurer.clone()],
                                &[&owner],
                                &[],
                                Some((4, PercolatorError::EngineLockActive)),
                            ));
                            rollbacks += 1;
                            book.check(&world);
                        }
                        peak = peak.max(land(
                            &mut world,
                            &[ix],
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
                    peak = peak.max(land(
                        &mut world,
                        &[principal],
                        &[],
                        &[market, vault, provider_token],
                        None,
                    ));
                    book.principal_paid = PRINCIPAL_PREFIX;
                    book.check(&world);
                    let admin_token = source;
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
                            AccountMeta::new(admin_token, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(world.env.mint, false),
                        ],
                    };
                    // The empty asset-0 scan is a separate bounded prefix.
                    peak = peak.max(land(
                        &mut world,
                        &[close.clone()],
                        &[&admin],
                        &[market],
                        None,
                    ));
                    book.check(&world);
                    world.env.svm.warp_to_slot(EXPIRY + schedule as u64);
                    let partial = reserve(&world, beneficiary, domain, false, 37);
                    peak = peak.max(land(
                        &mut world,
                        &[close.clone(), partial.clone(), denied],
                        &[&admin],
                        &[],
                        Some((4, PercolatorError::ExpectedSigner)),
                    ));
                    rollbacks += 1;
                    book.check(&world);
                    peak = peak.max(land(
                        &mut world,
                        &[close.clone()],
                        &[&admin],
                        &[market],
                        None,
                    ));
                    book.expired = true;
                    book.check(&world);
                    for amount in [37, book.recovered() - 37] {
                        let payment = reserve(&world, beneficiary, domain, false, amount);
                        peak = peak.max(land(
                            &mut world,
                            &[payment],
                            &[],
                            &[market, vault, beneficiary_token],
                            None,
                        ));
                        book.recredited = true;
                        book.insurance_paid += amount;
                        book.check(&world);
                    }
                    let result = book.check(&world);
                    if let Some(expected) = normalized {
                        assert_eq!(
                            result, expected,
                            "close order preserves every owner's combined entitlement"
                        );
                    } else {
                        normalized = Some(result);
                    }
                    assert_eq!(world.env.token_amount(vault), 0);
                    for _ in 0..8 {
                        if world.env.svm.get_account(&market).unwrap().data.len()
                            == percolator_prog::constants::HEADER_LEN
                        {
                            break;
                        }
                        let mint = world.env.mint;
                        peak = peak.max(land(
                            &mut world,
                            &[close.clone()],
                            &[&admin],
                            &[market, vault, mint, admin.pubkey()],
                            None,
                        ));
                    }
                    assert_closed_market_tombstone(&world.env.svm.get_account(&market).unwrap());
                    assert_eq!(world.env.token_amount(admin_token), 0);
                    assert_eq!(
                        std::array::from_fn::<_, 5, _>(|actor| world
                            .env
                            .token_amount(world.actors[actor].token)
                            as u128),
                        result
                    );
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 24);
    eprintln!("pending reserve role recredit: {worlds} worlds, {rollbacks} exact rollbacks, 120 user payouts/deletions, 24 terminal closes; peak={peak} CU");
}
