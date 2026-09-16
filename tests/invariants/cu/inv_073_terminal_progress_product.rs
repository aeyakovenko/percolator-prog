//! Scope H: finite terminal payment/expiry product with unavailable reserve keys.
//! The input-owned book separates source/provider principal, earned reserves,
//! available insurance, recoverable spend and expired residue on classic/native
//! quote rails. User settlement is reused setup, not a new seniority theorem.
//! Native booked-residue endpoints remain explicit administrative coverage gaps.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::account::Account;

#[path = "inv_070_native_booked_residue_cleanup.rs"]
mod native_booked_residue_cleanup;

#[path = "inv_073_booked_residue_beneficiary_epochs.rs"]
mod booked_residue_beneficiary_epochs;

const LIMIT: u64 = 300_000;
const INITIAL_STOCK: u64 = SOURCE_PRINCIPAL + BACKING + PROVIDER_FEE + AVAILABLE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Principal,
    Earnings,
    Insurance,
    Expiry,
}

fn payment_words() -> Vec<Vec<Event>> {
    fn visit(prefix: &mut Vec<Event>, words: &mut Vec<Vec<Event>>) {
        let events = [
            Event::Principal,
            Event::Earnings,
            Event::Insurance,
            Event::Expiry,
        ];
        if prefix.len() == events.len() {
            words.push(prefix.clone());
            return;
        }
        for event in events {
            // The selected finite family retains exactly the chosen residual
            // at expiry. The principal withdrawal therefore precedes expiry.
            if prefix.contains(&event)
                || (event == Event::Expiry && !prefix.contains(&Event::Principal))
            {
                continue;
            }
            prefix.push(event);
            visit(prefix, words);
            prefix.pop();
        }
    }
    let mut words = Vec::new();
    visit(&mut Vec::new(), &mut words);
    assert_eq!(words.len(), 12);
    for (index, word) in words.iter().enumerate() {
        assert!(!words[..index].contains(word));
    }
    words
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    stock: [u64; 4],
    recipients: [u64; 5],
    custody: u64,
    spent: u64,
}

struct Book {
    residual: u64,
    paid: [u64; 4],
    expired: bool,
    recredited: u64,
}

impl Book {
    fn recovery(&self) -> u64 {
        self.residual.min(SPENT)
    }

    fn expected(&self) -> Observation {
        Observation {
            stock: [
                SOURCE_PRINCIPAL - self.paid[0],
                if self.expired {
                    0
                } else {
                    BACKING - self.paid[1]
                },
                PROVIDER_FEE - self.paid[2],
                AVAILABLE + self.recredited - self.paid[3],
            ],
            recipients: [
                USER_PAID[0],
                USER_PAID[1],
                self.paid[..3].iter().sum(),
                0,
                self.paid[3],
            ],
            custody: INITIAL_STOCK - self.paid.iter().sum::<u64>(),
            spent: SPENT - self.recredited,
        }
    }

    fn accepts(&self, observed: &Observation) -> Result<(), &'static str> {
        let expected = self.expected();
        if observed.stock != expected.stock || observed.spent != expected.spent {
            return Err("terminal stock class or historical spend");
        }
        if observed.recipients != expected.recipients {
            return Err("terminal recipient entitlement");
        }
        if observed.custody != expected.custody {
            return Err("terminal physical custody");
        }
        Ok(())
    }

    fn rank(&self) -> (u8, u64) {
        let pending_recovery = if self.expired {
            self.recovery() - self.recredited
        } else {
            0
        };
        (
            u8::from(!self.expired),
            self.expected().stock.iter().sum::<u64>() + pending_recovery,
        )
    }

    fn pay(&mut self, class: usize, amount: u64) {
        let rank = self.rank();
        if class == 3 && self.expired {
            self.recredited = self.recovery();
        }
        assert!(amount > 0 && amount <= self.expected().stock[class]);
        self.paid[class] += amount;
        assert_eq!(self.rank(), (rank.0, rank.1 - amount));
    }

    fn expire(&mut self) {
        assert!(!self.expired);
        assert_eq!(BACKING - self.paid[1], self.residual);
        let rank = self.rank();
        self.expired = true;
        assert!(self.rank() < rank);
    }
}

fn token_image(frame: &Account, amount: u64) -> Account {
    let mut expected = frame.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    if token.is_native.is_some() {
        expected.lamports = expected.lamports - token.amount + amount;
    }
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

struct Frames {
    wallets: [Option<Account>; 5],
    tokens: [Account; 5],
    absent_tokens: [Option<Account>; 5],
    vault: Account,
    market: Account,
    ledger: Account,
    mint: Account,
}

impl Frames {
    fn check(
        &self,
        env: &V16CuEnv,
        book: &Book,
        wallets: [Pubkey; 5],
        tokens: [Pubkey; 5],
        ledger: Pubkey,
        created: [bool; 5],
    ) {
        let group = env.market_state().1;
        let atom = |num: u128| {
            assert_eq!(num % BOUND_SCALE, 0);
            u64::try_from(num / BOUND_SCALE).unwrap()
        };
        let observed = Observation {
            stock: [
                atom(group.source_backing_buckets[0].fresh_unliened_backing_num),
                atom(group.source_backing_buckets[1].fresh_unliened_backing_num),
                u64::try_from(group.backing_provider_earnings_total).unwrap(),
                u64::try_from(group.insurance).unwrap(),
            ],
            recipients: std::array::from_fn(|actor| {
                if created[actor] {
                    env.token_amount(tokens[actor])
                } else {
                    0
                }
            }),
            custody: env.token_amount(env.vault),
            spent: u64::try_from(group.insurance_domain_spent[1]).unwrap(),
        };
        book.accepts(&observed).unwrap();
        let expected = book.expected();
        assert_eq!(
            expected.recipients.iter().sum::<u64>() + expected.custody,
            SUPPLY
        );
        assert_eq!(group.vault, u128::from(expected.custody));
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.insurance_domain_spent[0], 0);
        let long_paid = book.paid[3].min(INSURANCE);
        assert_eq!(
            group.insurance_domain_budget,
            [
                u128::from(INSURANCE - long_paid),
                u128::from(INSURANCE_FEE - (book.paid[3] - long_paid)),
            ]
        );
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            group.insurance
        );
        for domain in 0..2 {
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(
                group.source_credit[domain].fresh_reserved_backing_num,
                u128::from(expected.stock[domain]) * BOUND_SCALE
            );
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(
                bucket.status,
                if expected.stock[domain] == 0 {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
        }
        assert_eq!(
            group.source_credit[0].provider_receivable_num,
            u128::from(SOURCE_PAID) * BOUND_SCALE
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(expected.stock[2])
        );
        assert_eq!(
            env.svm.get_account(&env.vault),
            Some(token_image(&self.vault, expected.custody))
        );
        for actor in 0..5 {
            let image = if created[actor] {
                Some(token_image(&self.tokens[actor], expected.recipients[actor]))
            } else {
                assert_eq!(expected.recipients[actor], 0);
                self.absent_tokens[actor].clone()
            };
            assert_eq!(env.svm.get_account(&tokens[actor]), image);
        }
        assert_eq!(wallets.map(|key| env.svm.get_account(&key)), self.wallets);
        assert_eq!(env.svm.get_account(&env.mint), Some(self.mint.clone()));
        let market = env.svm.get_account(&env.market).unwrap();
        assert_eq!(market.lamports, self.market.lamports);
        assert_eq!(
            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
            state::read_asset_oracle_profile(&self.market.data, 0).unwrap()
        );
        assert_eq!(
            state::read_asset_control_sequences(&market.data, 0).unwrap(),
            state::read_asset_control_sequences(&self.market.data, 0).unwrap()
        );
        assert_market_stock_census(
            "terminal progress product",
            &group,
            &market.data,
            &[],
            expected.custody.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("terminal progress product", &group, &[]).unwrap();
        state::market_view_mut(&mut market.data.clone())
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
        let account = env.svm.get_account(&ledger).unwrap();
        if book.paid[2] == 0 {
            assert_eq!(account, self.ledger);
        } else {
            let record = state::read_backing_domain_ledger(&account.data).unwrap();
            assert_eq!(record.market_group, env.market.to_bytes());
            assert_eq!(record.authority, wallets[2].to_bytes());
            assert_eq!(
                record.total_earnings_withdrawn_atoms,
                u128::from(book.paid[2])
            );
            assert_eq!(
                record.last_observed_bucket_earnings_atoms,
                u128::from(expected.stock[2])
            );
            assert_eq!(account.lamports, self.ledger.lamports);
        }
    }
}

#[derive(Default)]
struct Counts {
    worlds: usize,
    payments: usize,
    repairs: usize,
    rollbacks: usize,
    retirements: usize,
    residue_endpoints: usize,
    insurance_after_expiry: usize,
    fees_after_recredit: usize,
    peak: u64,
}

fn run_product(native: bool) {
    assert_eq!(
        (PROVIDER_FEE, SPENT, AVAILABLE, SOURCE_PRINCIPAL),
        (657, 73, 176, 1)
    );
    let words = payment_words();
    let mut counts = Counts::default();
    let mut outcomes = std::collections::BTreeMap::new();
    for residual in [17, SPENT, 101] {
        for expiry_slot in [100, 101] {
            for absent_mask in 0..8 {
                for word in &words {
                    let (world, insurer, admin_token) = terminal_fee_loss_world_with_quote(native);
                    let TerminalEarningsWorld {
                        mut env,
                        admin,
                        incumbent,
                        successor,
                        wallets,
                        tokens,
                        portfolios,
                        mint_frame,
                    } = world;
                    let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
                    let mut created = [true; 5];
                    for (bit, (actor, signer)) in [(2, &incumbent), (3, &successor), (4, &insurer)]
                        .into_iter()
                        .enumerate()
                    {
                        if absent_mask & (1 << bit) == 0 {
                            continue;
                        }
                        assert_eq!(env.token_amount(tokens[actor]), 0);
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::close_account(
                                &spl_token::ID,
                                &tokens[actor],
                                &wallets[actor],
                                &wallets[actor],
                                &[],
                            )
                            .unwrap(),
                            &[signer],
                        )
                        .unwrap();
                        let balance = env.svm.get_account(&wallets[actor]).unwrap().lamports;
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            system_instruction::transfer(
                                &wallets[actor],
                                &env.payer.pubkey(),
                                balance,
                            ),
                            &[signer],
                        )
                        .unwrap();
                        for key in [tokens[actor], wallets[actor]] {
                            assert!(env.svm.get_account(&key).is_none_or(|account| account
                                .lamports
                                == 0
                                && account.data.is_empty()
                                && account.owner == solana_sdk::system_program::ID
                                && !account.executable));
                        }
                        created[actor] = false;
                    }
                    // Retain the insurer key; terminal insurance payout is signer-gated.
                    drop((incumbent, successor));
                    assert!(!wallets.contains(&env.payer.pubkey()));
                    assert!(!wallets.contains(&admin.pubkey()));
                    let ledger = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &ledger,
                        state::backing_domain_ledger_account_len(),
                        env.program_id,
                    );
                    let ledger = ledger.pubkey();
                    let frames = Frames {
                        wallets: wallets.map(|key| env.svm.get_account(&key)),
                        tokens: token_frames,
                        absent_tokens: tokens.map(|key| env.svm.get_account(&key)),
                        vault: env.svm.get_account(&env.vault).unwrap(),
                        market: env.svm.get_account(&env.market).unwrap(),
                        ledger: env.svm.get_account(&ledger).unwrap(),
                        mint: mint_frame,
                    };
                    let tracked = [
                        env.market,
                        env.vault,
                        env.mint,
                        ledger,
                        admin.pubkey(),
                        admin_token,
                    ]
                    .into_iter()
                    .chain(wallets)
                    .chain(tokens)
                    .chain(portfolios)
                    .collect::<Vec<_>>();
                    let close = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new(admin_token, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                            AccountMeta::new(env.mint, false),
                        ],
                        data: ProgInstruction::CloseSlab {
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        }
                        .encode(),
                    };
                    let mut denied = close.clone();
                    denied.accounts[0] = AccountMeta::new(wallets[3], false);
                    let mut book = Book {
                        residual,
                        paid: [0; 4],
                        expired: false,
                        recredited: 0,
                    };
                    let mut wrong_class = book.expected();
                    wrong_class.stock[1] -= 1;
                    wrong_class.stock[2] += 1;
                    assert!(book.accepts(&wrong_class).is_err());
                    let mut wrong_owner = book.expected();
                    wrong_owner.recipients[1] -= 1;
                    wrong_owner.recipients[2] += 1;
                    assert!(book.accepts(&wrong_owner).is_err());
                    let check = |env: &V16CuEnv, book: &Book, created| {
                        frames.check(env, book, wallets, tokens, ledger, created);
                        assert_eq!(env.token_amount(admin_token), 0);
                    };
                    check(&env, &book, created);
                    let recovery = book.recovery();
                    // A one-atom source remainder is a distinct principal class.
                    // Two final payments exercise one-time recredit with an unpaid tail.
                    let actions = [(None, 0usize, SOURCE_PRINCIPAL)]
                        .into_iter()
                        .chain(word.iter().map(|event| match event {
                            Event::Principal => (Some(*event), 1, BACKING - residual),
                            Event::Earnings => (Some(*event), 2, PROVIDER_FEE),
                            Event::Insurance => (Some(*event), 3, AVAILABLE),
                            Event::Expiry => (Some(*event), 0, 0),
                        }))
                        .chain([(None, 3, recovery / 2), (None, 3, recovery - recovery / 2)]);
                    let mut calls = 0;
                    for (event, class, amount) in actions {
                        let mut batch = Vec::new();
                        let mut allowed = vec![env.market];
                        let mut rent = 0;
                        let mut signers = Vec::new();
                        let actor = if class == 3 { 4 } else { 2 };
                        if event == Some(Event::Expiry) {
                            env.svm.warp_to_slot(expiry_slot);
                            batch.push(close.clone());
                            signers.push(&admin);
                        } else {
                            if !created[actor] {
                                batch.push(Instruction {
                                    program_id: associated_token_program_id(),
                                    accounts: vec![
                                        AccountMeta::new(env.payer.pubkey(), true),
                                        AccountMeta::new(tokens[actor], false),
                                        AccountMeta::new_readonly(wallets[actor], false),
                                        AccountMeta::new_readonly(env.mint, false),
                                        AccountMeta::new_readonly(
                                            solana_sdk::system_program::ID,
                                            false,
                                        ),
                                        AccountMeta::new_readonly(spl_token::ID, false),
                                    ],
                                    data: vec![1],
                                });
                                rent = env
                                    .svm
                                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                            }
                            let kind = class.saturating_sub(1);
                            let mut payout =
                                reserve_payout(&env, wallets, tokens, ledger, kind, amount);
                            if class == 0 {
                                payout.data = ProgInstruction::WithdrawBackingBucket {
                                    domain: 0,
                                    market_id: env.asset_market_id(0),
                                    authority_epoch: env.control_sequences(0).authority_epoch,
                                    amount: amount.into(),
                                }
                                .encode();
                            }
                            if class == 3 {
                                payout.accounts[0].is_signer = true;
                                signers.push(&insurer);
                            }
                            assert_eq!(
                                payout.accounts.iter().any(|meta| meta.is_signer),
                                class == 3
                            );
                            batch.push(payout);
                            allowed.extend([env.vault, tokens[actor]]);
                            if class == 2 {
                                allowed.push(ledger);
                            }
                        }
                        let mut rejected = batch.clone();
                        rejected.push(denied.clone());
                        counts.peak = counts.peak.max(land(
                            &mut env,
                            &rejected,
                            &signers,
                            &tracked,
                            &[],
                            0,
                            None,
                            Some((2 + batch.len() as u8, PercolatorError::ExpectedSigner)),
                        ));
                        counts.rollbacks += 1;
                        check(&env, &book, created);
                        counts.peak = counts.peak.max(land(
                            &mut env, &batch, &signers, &tracked, &allowed, rent, None, None,
                        ));
                        if event == Some(Event::Expiry) {
                            book.expire();
                        } else {
                            counts.insurance_after_expiry +=
                                usize::from(class == 3 && book.expired);
                            counts.fees_after_recredit +=
                                usize::from(class == 2 && book.recredited > 0);
                            book.pay(class, amount);
                            counts.repairs += usize::from(!created[actor]);
                            created[actor] = true;
                            counts.payments += 1;
                        }
                        calls += 1;
                        check(&env, &book, created);
                    }
                    assert_eq!(calls, 7);
                    assert_eq!(book.rank(), (0, 0));
                    assert_eq!(
                        book.paid,
                        [
                            SOURCE_PRINCIPAL,
                            BACKING - residual,
                            PROVIDER_FEE,
                            AVAILABLE + recovery
                        ]
                    );
                    assert_eq!(book.expected().custody, residual - recovery);
                    if let Some(previous) = outcomes.insert(residual, book.expected()) {
                        assert_eq!(book.expected(), previous,
                            "wallet availability, payment order and exact/late expiry preserve entitlements");
                    }
                    counts.worlds += 1;
                    if native && residual > recovery {
                        // Matches the declared N/X/B boundary: native booked-stock
                        // retirement is unverified. Economic claims are exhausted.
                        counts.residue_endpoints += 1;
                        continue;
                    }
                    let final_tokens = tokens.map(|key| env.svm.get_account(&key));
                    let final_ledger = env.svm.get_account(&ledger);
                    let tombstone_rent = env
                        .svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                    let refund = frames.market.lamports
                        + env.svm.get_account(&env.vault).unwrap().lamports
                        - tombstone_rent;
                    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                    expected_admin.lamports += refund;
                    let allowed = [env.market, env.vault, env.mint];
                    let mut rejected = vec![close.clone(), denied.clone()];
                    counts.peak = counts.peak.max(land(
                        &mut env,
                        &rejected,
                        &[&admin],
                        &tracked,
                        &[],
                        0,
                        None,
                        Some((3, PercolatorError::ExpectedSigner)),
                    ));
                    counts.rollbacks += 1;
                    check(&env, &book, created);
                    rejected.pop();
                    counts.peak = counts.peak.max(land(
                        &mut env,
                        &rejected,
                        &[&admin],
                        &tracked,
                        &allowed,
                        0,
                        Some((admin.pubkey(), refund)),
                        None,
                    ));
                    let tombstone = env.svm.get_account(&env.market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(tombstone.lamports, tombstone_rent);
                    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                    assert!(env
                        .svm
                        .get_account(&env.vault)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                    assert_eq!(tokens.map(|key| env.svm.get_account(&key)), final_tokens);
                    assert_eq!(env.svm.get_account(&ledger), final_ledger);
                    assert_eq!(wallets.map(|key| env.svm.get_account(&key)), frames.wallets);
                    assert_eq!(env.token_amount(admin_token), 0);
                    let mut expected_mint = frames.mint;
                    if !native {
                        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                        mint.supply -= residual - recovery;
                        Mint::pack(mint, &mut expected_mint.data).unwrap();
                    }
                    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
                    counts.retirements += 1;
                }
            }
            eprintln!("Scope H progress: native={native}, residual={residual}, expiry={expiry_slot}, worlds={}, peak={} CU",
                counts.worlds, counts.peak);
        }
    }
    assert_eq!(counts.worlds, 576);
    assert_eq!(counts.payments, 6 * counts.worlds);
    assert_eq!(counts.repairs, counts.worlds);
    assert_eq!(counts.retirements + counts.residue_endpoints, counts.worlds);
    assert_eq!(counts.residue_endpoints, if native { 192 } else { 0 });
    assert_eq!(counts.rollbacks, 7 * counts.worlds + counts.retirements);
    assert!(counts.insurance_after_expiry > 2 * counts.worlds);
    assert!(counts.fees_after_recredit > 0);
    assert_cu_within("Scope H terminal progress product", counts.peak, LIMIT);
    eprintln!("Scope H terminal product: native={native}, worlds={}, payments={}, repairs={}, exact_rollbacks={}, retirements={}, native_residue_endpoints={}, insurance_after_expiry={}, fees_after_recredit={}, peak={} CU",
        counts.worlds, counts.payments, counts.repairs, counts.rollbacks, counts.retirements,
        counts.residue_endpoints, counts.insurance_after_expiry, counts.fees_after_recredit, counts.peak);
}

#[test]
fn v16_program_classic_terminal_progress_product_preserves_stock_and_recredit() {
    run_product(false);
}

#[test]
fn v16_program_native_terminal_progress_product_preserves_stock_and_recredit() {
    run_product(true);
}
