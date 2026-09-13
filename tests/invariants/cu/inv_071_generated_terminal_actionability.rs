//! Generated receipt completion, departed beneficiaries and repeated terminal recredit.
//! The oracle selects public work from individual obligations, not the engine selector.

use super::*;
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, RngAlgorithm, TestRng, TestRunner},
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CHUNK: usize = percolator::TERMINAL_SLAB_SCAN_ASSETS_PER_CALL;
const START: u64 = 10_000;
const RESOLVE: u64 = START + 40;
const RECEIPT_EXPIRY: u64 = START + 45;
const LIMIT: u64 = 900_000;
const CAPITAL: [u64; 8] = [1_000, 100, 1_000, 1_000, 100, 1_000, 20, 137];
// Uninsured loss settlement halves the two junior sources; asset zero is insured.
const FACES: [u64; 8] = [100, 0, 30, 70, 0, 20, 0, 0];
const PAID: [u64; 8] = [1_200, 0, 1_030, 1_070, 0, 1_020, 0, 137];
const SPENT: u64 = 100;
const INSURANCE: u64 = 300;
const INITIAL_INSURANCE: u64 = INSURANCE - SPENT;
const RECEIPT_BACKING: u64 = 17;

#[derive(Clone, Debug)]
struct History {
    first: usize,
    last: usize,
    backing: [u64; 2],
    deadlines: [u64; 2],
    provider_prefix: u64,
    chunk: u64,
    reverse: bool,
    crank: bool,
}

impl History {
    fn slots(&self) -> usize {
        self.last + 1
    }

    fn supply(&self) -> u64 {
        CAPITAL.iter().sum::<u64>()
            + INSURANCE
            + 1
            + RECEIPT_BACKING
            + self.backing.iter().sum::<u64>()
    }

    fn order(&self, mut actors: Vec<usize>) -> Vec<usize> {
        if self.reverse {
            actors.reverse();
        }
        actors
    }
}

struct World {
    env: V16CuEnv,
    owners: [Pubkey; 8],
    portfolios: [Pubkey; 8],
    tokens: [Pubkey; 8],
    insurer: Pubkey,
    provider: Pubkey,
    reserve: Pubkey,
    provider_token: Pubkey,
    destination: Pubkey,
    tracked: Vec<Pubkey>,
    peak: u64,
    steps: usize,
    rollbacks: usize,
}

impl World {
    fn wire(&self, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: ix.encode(),
        }
    }

    fn new(h: &History) -> Self {
        use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;
        let mut env = inv018_public_spl_market_with_capacity(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 3,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
            h.slots(),
        );
        let admin = env.admin.insecure_clone();
        let insurer = Keypair::new();
        let provider = Keypair::new();
        for key in [&insurer, &provider] {
            env.svm.airdrop(&key.pubkey(), 1_000_000_000).unwrap();
        }
        let mut peak = env.init_market_cu;
        for asset in 3..h.slots() {
            peak = peak.max(env.activate_asset_with_authorities(
                asset as u16,
                asset as u64,
                100,
                insurer.pubkey(),
                admin.pubkey(),
                provider.pubkey(),
                admin.pubkey(),
            ));
        }
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(&insurer),
            0,
            processor::ASSET_AUTH_INSURANCE,
            insurer.pubkey().to_bytes(),
        )
        .unwrap();
        for asset in [1, 2] {
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&provider),
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                provider.pubkey().to_bytes(),
            )
            .unwrap();
        }
        env.svm.warp_to_slot(START);
        for asset in 0..3 {
            env.configure_auth_mark_for_asset_as_admin(asset, START, 100);
        }
        let owners: [Keypair; 8] = std::array::from_fn(|_| Keypair::new());
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap();
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        let reserve = create_ata_for_test(&mut env.svm, &env.payer, insurer.pubkey(), env.mint);
        let provider_token =
            create_ata_for_test(&mut env.svm, &env.payer, provider.pubkey(), env.mint);
        let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        for (token, amount) in tokens.into_iter().zip(CAPITAL).chain([
            (reserve, INSURANCE),
            (
                provider_token,
                1 + RECEIPT_BACKING + h.backing.iter().sum::<u64>(),
            ),
        ]) {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &admin.pubkey(),
                    &[],
                    amount,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
        }
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
        for actor in 0..8 {
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL[actor].into()),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
        }
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                amount: INSURANCE.into(),
            },
            vec![
                AccountMeta::new(insurer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(reserve, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&insurer],
        )
        .unwrap();
        for (domain, amount, expiry) in [
            (3, 1, RESOLVE),
            (5, RECEIPT_BACKING, RECEIPT_EXPIRY),
            (2 * h.first, h.backing[0], h.deadlines[0]),
            (2 * h.last + 1, h.backing[1], h.deadlines[1]),
        ] {
            let asset = (domain / 2) as u16;
            env.svm.expire_blockhash();
            env.send(
                ProgInstruction::TopUpBackingBucket {
                    domain: domain as u16,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: env.control_sequences(usize::from(asset)).authority_epoch,
                    intent_id: 0,
                    backing_fee_bps: 0,
                    insurance_share_bps: 0,
                    amount: amount.into(),
                    expiry_slot: expiry,
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(provider_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&provider],
            )
            .unwrap();
        }
        for (winner, loser, asset, size) in
            [(0, 1, 0, 10), (2, 4, 1, 3), (3, 4, 1, 7), (5, 6, 2, 2)]
        {
            env.svm.expire_blockhash();
            env.trade_asset_with_cu(
                asset,
                &owners[winner],
                portfolios[winner],
                &owners[loser],
                portfolios[loser],
                size * POS_SCALE as i128,
                100,
                0,
            );
        }
        for offset in 0..5 {
            let slot = START + offset + 1;
            env.svm.warp_to_slot(slot);
            for asset in 0..3 {
                env.push_auth_mark_for_asset_as_admin(asset, slot, 100 + 5 * (offset + 1).min(4));
            }
            env.crank(
                portfolios[7],
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations_for_assets(&[0, 1, 2]),
                },
            );
        }
        for actor in [0, 1, 2, 3, 4, 5, 6] {
            env.svm.expire_blockhash();
            env.crank(
                portfolios[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: START + 5,
                    observations: crank_observations_for_assets(&[0, 1, 2]),
                },
            );
        }
        for (actor, face) in [(0, 200), (2, 60), (3, 140), (5, 40)] {
            assert_eq!(env.portfolio_state(portfolios[actor]).pnl.get(), face);
        }
        env.svm.warp_to_slot(RESOLVE);
        peak = peak.max(env.resolve());
        env.svm.warp_to_slot(RESOLVE + 3);
        let owners = owners.each_ref().map(Signer::pubkey);
        let insurer = insurer.pubkey();
        let provider = provider.pubkey();
        let mut tracked = vec![
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            insurer,
            provider,
            reserve,
            provider_token,
            destination,
            admin.pubkey(),
        ];
        tracked.extend(owners);
        tracked.extend(portfolios);
        tracked.extend(tokens);
        Self {
            env,
            owners,
            portfolios,
            tokens,
            insurer,
            provider,
            reserve,
            provider_token,
            destination,
            tracked,
            peak,
            steps: 0,
            rollbacks: 0,
        }
    }

    fn land(
        &mut self,
        ixs: &[Instruction],
        admin: bool,
        rejection: Option<(usize, InstructionError)>,
    ) {
        self.env.svm.expire_blockhash();
        let mut instructions = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        ];
        instructions.extend_from_slice(ixs);
        let mut signers = vec![&self.env.payer];
        if admin {
            signers.push(&self.env.admin);
        }
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        assert_eq!(
            tx.message.header.num_required_signatures,
            if admin { 2 } else { 1 }
        );
        let mut keys = tx.message.account_keys.clone();
        keys.extend(&self.tracked);
        keys.sort_unstable();
        keys.dedup();
        let mut before: Vec<_> = keys
            .iter()
            .map(|key| self.env.svm.get_account(key))
            .collect();
        let payer = keys
            .iter()
            .position(|key| *key == self.env.payer.pubkey())
            .unwrap();
        before[payer].as_mut().unwrap().lamports -=
            u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
        let result = self.env.svm.send_transaction(tx);
        let meta = if let Some((index, error)) = rejection {
            let failure = result.expect_err("explicit wait or caller-funded suffix");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index as u8, error)
            );
            assert_eq!(
                keys.iter()
                    .map(|key| self.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );
            if index > 2 {
                assert!(failure
                    .meta
                    .logs
                    .iter()
                    .any(|line| *line == format!("Program {} success", self.env.program_id)));
            }
            self.rollbacks += 1;
            failure.meta
        } else {
            self.steps += 1;
            let meta = result.expect("oracle-selected public continuation");
            for (key, account) in keys.iter().zip(before) {
                if *key == self.env.payer.pubkey()
                    || !ixs.iter().any(|ix| {
                        ix.accounts
                            .iter()
                            .any(|meta| meta.pubkey == *key && meta.is_writable)
                    })
                {
                    assert_eq!(
                        self.env.svm.get_account(key),
                        account,
                        "complete peer Account frame"
                    );
                }
            }
            meta
        };
        assert_cu_within(
            "generated terminal actionability",
            meta.compute_units_consumed,
            LIMIT,
        );
        self.peak = self.peak.max(meta.compute_units_consumed);
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        let vault = self
            .env
            .svm
            .get_account(&self.env.vault)
            .filter(|account| account.lamports != 0)
            .map_or(0, |account| {
                TokenAccount::unpack(&account.data).unwrap().amount
            });
        let wallets = self
            .tokens
            .into_iter()
            .chain([self.reserve, self.provider_token, self.destination])
            .map(|key| self.env.token_amount(key))
            .sum::<u64>();
        assert_eq!(
            mint.supply,
            vault + wallets,
            "no untracked token disposition"
        );
        if self
            .env
            .svm
            .get_account(&self.env.market)
            .unwrap()
            .data
            .len()
            > percolator_prog::constants::HEADER_LEN
        {
            assert_eq!(self.env.market_state().1.vault, u128::from(vault));
        }
    }

    fn payout(&self, actor: usize, topup: bool, crank: bool) -> Instruction {
        self.wire(
            if topup {
                ProgInstruction::ClaimResolvedPayoutTopup
            } else if crank {
                ProgInstruction::PermissionlessCrank {
                    now_slot: self.env.svm.get_sysvar::<Clock>().slot,
                    observations: vec![],
                }
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            },
            vec![
                AccountMeta::new_readonly(self.owners[actor], false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }

    fn scan(&self) -> Instruction {
        self.wire(
            ProgInstruction::CloseSlab {
                authority_epoch: self.env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(self.env.admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new(self.destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(self.env.mint, false),
            ],
        )
    }

    fn insurance(&self, amount: u64) -> Instruction {
        self.wire(
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 0,
                market_id: self.env.asset_market_id(0),
                authority_epoch: self.env.control_sequences(0).authority_epoch,
                amount: amount.into(),
            },
            vec![
                AccountMeta::new_readonly(self.insurer, false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.reserve, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }
}

// Obligations are decoded from individual accounts, not the engine's action selector.
// Earlier components may fund later components, which is why the rank is lexicographic.
fn user_rank(world: &World, actor: usize) -> [u128; 8] {
    let account = world.env.portfolio_state(world.portfolios[actor]);
    let receipt = resolved_receipt(&account);
    let group = world.env.market_state().1;
    [
        [3, 5]
            .into_iter()
            .filter(|domain| {
                group.source_backing_buckets[*domain].status == BackingBucketStatusV16::Fresh
            })
            .count() as u128,
        u128::from(percolator::active_bitmap_count_ones(active_bitmap(
            &account,
        ))),
        account
            .source_domains
            .iter()
            .map(|source| source.source_claim_liened_num.get())
            .sum(),
        account
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count() as u128,
        account.pnl.get().min(0).unsigned_abs(),
        account.capital.get() + account.pnl.get().max(0) as u128,
        u128::from(
            PAID[actor]
                .checked_sub(world.env.token_amount(world.tokens[actor]))
                .unwrap(),
        ),
        u128::from(RESOLVE.saturating_sub(account.last_fee_slot.get()))
            + u128::from(receipt.present && !receipt.finalized),
    ]
}

fn advance_user(world: &mut World, actor: usize, h: &History, receipt_only: bool) {
    for _ in 0..16 {
        let account = world.env.portfolio_state(world.portfolios[actor]);
        if (receipt_only && resolved_receipt(&account).present)
            || (!receipt_only
                && resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]))
        {
            return;
        }
        let before = user_rank(world, actor);
        world.land(&[world.payout(actor, false, h.crank)], false, None);
        let after = user_rank(world, actor);
        assert!(
            after < before,
            "user {actor} must consume a local obligation: {before:?} -> {after:?}"
        );
    }
    panic!("user obligation rank did not terminate");
}

fn settle_receipts(world: &mut World, h: &History) {
    for actor in h.order(vec![1, 4, 6]) {
        advance_user(world, actor, h, false);
    }
    advance_user(world, 7, h, false);
    // Keep the insured winner exposed until the other positive accounts detach.
    // This preserves the third junior source for the later receipt-expiry schedule.
    for actor in [5, 2, 3] {
        for _ in 0..8 {
            if percolator::active_bitmap_is_empty(active_bitmap(
                &world.env.portfolio_state(world.portfolios[actor]),
            )) {
                break;
            }
            let before = user_rank(world, actor);
            world.land(&[world.payout(actor, false, h.crank)], false, None);
            assert!(user_rank(world, actor) < before);
        }
        assert!(percolator::active_bitmap_is_empty(active_bitmap(
            &world.env.portfolio_state(world.portfolios[actor])
        )));
        assert!(!resolved_receipt(&world.env.portfolio_state(world.portfolios[actor])).present);
        assert_eq!(
            world.env.portfolio_state(world.portfolios[actor]).pnl.get(),
            i128::from(FACES[actor])
        );
    }
    let source = world.env.market_state().1.source_credit[1];
    let converted = (200 * source.credit_rate_num / percolator::CREDIT_RATE_SCALE) as u64;
    assert_eq!(converted, CAPITAL[1]);
    let total_face = FACES.iter().sum::<u64>();
    for actor in std::iter::once(0).chain(h.order(vec![2, 3])) {
        advance_user(world, actor, h, true);
        let receipt = resolved_receipt(&world.env.portfolio_state(world.portfolios[actor]));
        let face = FACES[actor];
        assert!(receipt.present && !receipt.finalized);
        assert_eq!(receipt.terminal_positive_claim_face, face.into());
        assert_eq!(
            receipt.prior_bound_contribution_num,
            u128::from(face) * BOUND_SCALE
        );
        assert_eq!(
            receipt.paid_effective,
            u128::from(face) * 201 / u128::from(total_face),
            "ledger={:?}",
            world.env.market_state().1.resolved_payout_ledger
        );
        assert_eq!(
            world.env.token_amount(world.tokens[actor]),
            CAPITAL[actor] + if actor == 0 { converted } else { 0 } + (face * 201 / total_face)
        );
    }
    let ledger = world.env.market_state().1.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_residual, 201);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        20 * BOUND_SCALE
    );
    assert_eq!(ledger.terminal_claim_exact_receipts_num, 200 * BOUND_SCALE);
    // Schedule the third source's expiry before realizing it. This clock choice
    // does not claim that earlier source realization is unavailable.
    let before = world.env.svm.get_account(&world.env.market);
    world.env.svm.warp_to_slot(RECEIPT_EXPIRY);
    assert_eq!(world.env.svm.get_account(&world.env.market), before);
    advance_user(world, 5, h, false);
    for actor in h.order(vec![0, 2, 3]) {
        let before = user_rank(world, actor);
        world.land(&[world.payout(actor, true, false)], false, None);
        assert!(user_rank(world, actor) < before);
        let receipt = resolved_receipt(&world.env.portfolio_state(world.portfolios[actor]));
        assert_eq!(receipt.paid_effective, FACES[actor].into());
        assert!(receipt.finalized);
        advance_user(world, actor, h, false);
    }
    assert_eq!(world.tokens.map(|key| world.env.token_amount(key)), PAID);
    for actor in h.order((0..8).collect()) {
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.portfolios[actor]
        ));
        let peers = world.tokens.map(|key| world.env.svm.get_account(&key));
        world.land(&[world.payout(actor, true, false)], false, None);
        assert_eq!(
            world.tokens.map(|key| world.env.svm.get_account(&key)),
            peers
        );
        let portfolio = world.portfolios[actor];
        let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
        let market_lamports = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        let count = world.env.market_state().1.materialized_portfolio_count;
        let close = world.wire(
            world.env.close_portfolio_ix(portfolio),
            vec![
                AccountMeta::new(world.env.admin.pubkey(), true),
                AccountMeta::new(world.env.market, false),
                AccountMeta::new(portfolio, false),
            ],
        );
        world.land(&[close], true, None);
        assert_eq!(
            world.env.market_state().1.materialized_portfolio_count,
            count - 1
        );
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market_lamports + rent
        );
        assert!(world
            .env
            .svm
            .get_account(&portfolio)
            .is_none_or(|account| account.lamports == 0));
    }
}

#[derive(Clone, Debug)]
enum Action {
    Provider(u64),
    Insurance { restore: u64, pay: u64 },
    Wait(u64),
    Expire(usize),
    Scan(usize),
    Close,
}

#[derive(Clone, Debug)]
struct Oracle {
    expired: [bool; 2],
    provider_paid: u64,
    restored: u64,
    insurance_paid: u64,
    cursor: usize,
    clock: u64,
    closed: bool,
}

impl Oracle {
    fn new() -> Self {
        Self {
            expired: [false; 2],
            provider_paid: 0,
            restored: 0,
            insurance_paid: 0,
            cursor: 0,
            clock: RECEIPT_EXPIRY,
            closed: false,
        }
    }

    fn remaining(&self, h: &History, i: usize) -> u64 {
        h.backing[i] - if i == 0 { self.provider_paid } else { 0 }
    }

    fn fresh(&self, h: &History) -> u64 {
        (0..2)
            .filter(|i| !self.expired[*i])
            .map(|i| self.remaining(h, i))
            .sum()
    }

    fn vault(&self, h: &History) -> u64 {
        1 + RECEIPT_BACKING + INITIAL_INSURANCE + h.backing.iter().sum::<u64>()
            - self.provider_paid
            - self.insurance_paid
    }

    fn rank(&self, h: &History) -> [u64; 6] {
        if self.closed {
            return [0; 6];
        }
        [
            h.deadlines
                .iter()
                .filter(|deadline| **deadline > self.clock)
                .count() as u64,
            self.expired.iter().filter(|expired| !**expired).count() as u64,
            INITIAL_INSURANCE
                + SPENT
                    .min(1 + RECEIPT_BACKING + h.backing.iter().sum::<u64>() - h.provider_prefix)
                - self.insurance_paid,
            h.provider_prefix - self.provider_paid,
            (h.slots() - self.cursor) as u64,
            1,
        ]
    }

    fn next(&self, h: &History) -> Action {
        if self.provider_paid < h.provider_prefix {
            return Action::Provider(h.provider_prefix);
        }
        let residual = self.vault(h)
            - self.fresh(h)
            - (INITIAL_INSURANCE + self.restored - self.insurance_paid);
        let restore = residual.min(SPENT - self.restored).min(100);
        let available = INITIAL_INSURANCE + self.restored - self.insurance_paid + restore;
        if available != 0 {
            return Action::Insurance {
                restore,
                pay: available.min(h.chunk),
            };
        }
        // The read set includes the earlier insurer even when cursor has passed asset zero.
        if self.fresh(h) == 0 && residual == 0 {
            return Action::Close;
        }
        let end = (self.cursor + CHUNK).min(h.slots());
        if let Some(i) = (0..2).find(|i| !self.expired[*i] && [h.first, h.last][*i] < end) {
            let asset = [h.first, h.last][i];
            assert!(asset >= self.cursor);
            if h.deadlines[i] <= self.clock {
                return Action::Expire(i);
            }
            if asset == self.cursor {
                return Action::Wait(h.deadlines[i]);
            }
            return Action::Scan(asset);
        }
        if end < h.slots() {
            Action::Scan(end)
        } else {
            Action::Close
        }
    }

    fn apply(&mut self, _h: &History, action: &Action) {
        match *action {
            Action::Provider(amount) => self.provider_paid += amount,
            Action::Insurance { restore, pay } => {
                self.restored += restore;
                self.insurance_paid += pay;
            }
            Action::Wait(slot) => self.clock = slot,
            Action::Expire(i) => {
                self.expired[i] = true;
                self.cursor = 0;
            }
            Action::Scan(cursor) => self.cursor = cursor,
            Action::Close => self.closed = true,
        }
    }

    fn check(&self, world: &World, h: &History) {
        let env = &world.env;
        assert_eq!(world.tokens.map(|key| env.token_amount(key)), PAID);
        assert_eq!(env.token_amount(world.provider_token), self.provider_paid);
        assert_eq!(env.token_amount(world.reserve), self.insurance_paid);
        assert_eq!(env.token_amount(world.destination), 0);
        let market = env.svm.get_account(&env.market).unwrap();
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        if self.closed {
            assert_closed_market_tombstone(&market);
            assert_eq!(
                mint.supply,
                PAID.iter().sum::<u64>() + self.provider_paid + self.insurance_paid
            );
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0));
            return;
        }
        let (cfg, group) = env.market_state();
        assert_eq!(cfg.terminal_slab_scan_progress, self.cursor as u128);
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.materialized_portfolio_count,
                group.c_tot,
                group.pnl_pos_tot,
                group.source_claim_bound_total_num
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert!(group
            .assets
            .iter()
            .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
        assert_eq!(group.vault, self.vault(h).into());
        assert_eq!(env.token_amount(env.vault), self.vault(h));
        assert_eq!(
            group.insurance,
            (INITIAL_INSURANCE + self.restored - self.insurance_paid).into()
        );
        assert_eq!(mint.supply, h.supply());
        assert_eq!(
            PAID.iter().sum::<u64>() + self.provider_paid + self.insurance_paid + self.vault(h),
            h.supply()
        );
        let mut fresh = 0;
        for (domain, (bucket, source)) in group
            .source_backing_buckets
            .iter()
            .zip(&group.source_credit)
            .enumerate()
        {
            let i = [2 * h.first, 2 * h.last + 1]
                .iter()
                .position(|d| *d == domain);
            let amount = i
                .filter(|i| !self.expired[*i])
                .map_or(0, |i| u128::from(self.remaining(h, i)) * BOUND_SCALE);
            assert_eq!(bucket.fresh_unliened_backing_num, amount);
            assert_eq!(source.fresh_reserved_backing_num, amount);
            assert_eq!(source.positive_claim_bound_num, 0);
            assert_eq!(
                (
                    bucket.valid_liened_backing_num,
                    source.valid_liened_backing_num
                ),
                (0, 0)
            );
            if let Some(i) = i {
                assert_eq!(bucket.expiry_slot, h.deadlines[i]);
                assert_eq!(
                    bucket.status,
                    if self.expired[i] {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
            }
            assert_eq!(
                group.insurance_domain_budget[domain],
                if domain == 0 {
                    (INSURANCE - self.insurance_paid).into()
                } else {
                    0
                }
            );
            assert_eq!(
                group.insurance_domain_spent[domain],
                if domain == 0 {
                    (SPENT - self.restored).into()
                } else {
                    0
                }
            );
            fresh += amount;
        }
        assert_eq!(
            group.source_credit[1].provider_receivable_num,
            100 * BOUND_SCALE
        );
        let header = market_group_header_bytes(&market.data);
        assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
        assert_eq!(fresh, u128::from(self.fresh(h)) * BOUND_SCALE);
        assert_eq!(
            header.insurance_domain_budget_remaining_total.get(),
            group.insurance
        );
        let ledger = group.resolved_payout_ledger;
        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            u128::from(FACES.iter().sum::<u64>()) * BOUND_SCALE
        );
        assert_eq!(
            ledger.snapshot_residual,
            u128::from(221 + RECEIPT_BACKING)
                + (0..2)
                    .filter(|i| self.expired[*i])
                    .map(|i| u128::from(self.remaining(h, i)))
                    .sum::<u128>()
        );
        assert_eq!(
            ledger.current_payout_rate_num,
            ledger.current_payout_rate_den
        );
        crate::support::fuzz_model::assert_market_stock_census(
            "generated terminal actionability",
            &group,
            &market.data,
            &[],
            group.vault,
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "generated terminal actionability",
            &group,
            &[],
        )
        .unwrap();
        let mut image = market.data.clone();
        state::market_view_mut(&mut image)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    }
}

fn run(h: &History) -> (usize, usize, u64) {
    let mut world = World::new(h);
    settle_receipts(&mut world, h);
    let mut oracle = Oracle::new();
    oracle.check(&world, h);
    let mut expiry_recredits = 0;
    let bound = 3 * h.slots().div_ceil(CHUNK) + INSURANCE.div_ceil(h.chunk) as usize + 12;
    for _ in 0..bound {
        let action = oracle.next(h);
        let rank = oracle.rank(h);
        let scan = world.scan();
        if let Action::Wait(slot) = action {
            world.land(
                &[scan],
                true,
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                )),
            );
            let before = world.env.svm.get_account(&world.env.market);
            world.env.svm.warp_to_slot(slot);
            assert_eq!(world.env.svm.get_account(&world.env.market), before);
        } else {
            let ix = match action {
                Action::Provider(amount) => world.wire(
                    ProgInstruction::WithdrawBackingBucket {
                        domain: (2 * h.first) as u16,
                        market_id: world.env.asset_market_id(h.first as u16),
                        authority_epoch: world.env.control_sequences(h.first).authority_epoch,
                        amount: amount.into(),
                    },
                    vec![
                        AccountMeta::new_readonly(world.provider, false),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.provider_token, false),
                        AccountMeta::new(world.env.vault, false),
                        AccountMeta::new_readonly(world.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                ),
                Action::Insurance { restore, pay } => {
                    expiry_recredits +=
                        usize::from(restore > 0 && oracle.expired.iter().any(|expired| *expired));
                    world.insurance(pay)
                }
                _ => scan,
            };
            let admin = matches!(action, Action::Scan(_) | Action::Expire(_) | Action::Close);
            let market = world.env.svm.get_account(&world.env.market).unwrap();
            let vault = world.env.svm.get_account(&world.env.vault).unwrap();
            let mut admin_frame = world
                .env
                .svm
                .get_account(&world.env.admin.pubkey())
                .unwrap();
            if matches!(action, Action::Close)
                || matches!(action, Action::Insurance { restore, .. }
                    if restore > 0 && oracle.expired.iter().any(|expired| *expired))
            {
                let fail = system_instruction::transfer(
                    &world.env.admin.pubkey(),
                    &world.provider,
                    u64::MAX,
                );
                world.land(
                    &[ix.clone(), fail],
                    true,
                    Some((3, InstructionError::Custom(1))),
                );
                oracle.check(&world, h);
            }
            world.land(&[ix], admin, None);
            if matches!(action, Action::Close) {
                let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
                let rent = world
                    .env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                assert_eq!(tombstone.lamports, rent);
                admin_frame.lamports += market.lamports - rent + vault.lamports;
                assert_eq!(
                    world.env.svm.get_account(&world.env.admin.pubkey()),
                    Some(admin_frame)
                );
            }
        }
        oracle.apply(h, &action);
        oracle.check(&world, h);
        assert!(oracle.rank(h) < rank, "{action:?} must lower rank {rank:?}");
        if oracle.closed {
            assert!(
                expiry_recredits == 2,
                "both expiries must fund an earlier absent insurer"
            );
            return (world.steps, world.rollbacks, world.peak);
        }
    }
    panic!("generated terminal obligations did not terminate: {h:?}");
}

#[test]
fn v16_program_generated_receipts_and_reserves_have_constructible_terminal_progress() {
    let evidence = std::cell::RefCell::new((0usize, 0usize, 0u64, 0usize));
    let verify = |h: History| {
        let (steps, rollbacks, peak) = run(&h);
        let mut totals = evidence.borrow_mut();
        totals.0 += steps;
        totals.1 += rollbacks;
        totals.2 = totals.2.max(peak);
        totals.3 += 1;
        println!("terminal history: slots={}, first={}, reverse={}, crank={}, steps={steps}, rollbacks={rollbacks}, peak={peak} CU", h.slots(), h.first, h.reverse, h.crank);
    };
    for (i, first) in [CHUNK - 1, CHUNK, CHUNK + 1].into_iter().enumerate() {
        verify(History {
            first,
            last: first + 2,
            backing: [61, 137],
            deadlines: [START + 60, START + 65],
            provider_prefix: 17,
            chunk: 31,
            reverse: i == 1,
            crank: i != 0,
        });
    }
    verify(History {
        first: CHUNK,
        last: MAX_10M_MARKET_SLOTS - 1,
        backing: [31, 43],
        deadlines: [START + 65, START + 60],
        provider_prefix: 7,
        chunk: 23,
        reverse: true,
        crank: false,
    });
    let strategy = (
        prop::sample::select(vec![3usize, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK - 1]),
        1usize..=5,
        20u64..=75,
        1u64..=160,
        prop::array::uniform2(START + 50..=START + 70),
        1u64..=19,
        23u64..=79,
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(first, gap, one, two, deadlines, provider_prefix, chunk, reverse, crank)| History {
                first,
                last: first + gap,
                backing: [one, two],
                deadlines,
                provider_prefix,
                chunk,
                reverse,
                crank,
            },
        );
    TestRunner::new_with_rng(
        Config {
            cases: 12,
            max_shrink_iters: 32,
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/inv_071_generated_terminal_actionability.txt",
            ))),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0xc7; 32]),
    )
    .run(&strategy, |h| {
        verify(h);
        Ok(())
    })
    .unwrap();
    let totals = evidence.into_inner();
    println!(
        "INV-071 generated terminal actionability: {} worlds, {} steps, {} rollbacks, peak {} CU",
        totals.3, totals.0, totals.1, totals.2
    );
}
