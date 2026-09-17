//! INV-024/025/028/063/066/067/070/073: resolved debt above a source's remaining
//! backing need belongs to junior residual. Carry that partition into actual receipts,
//! source conversion, late expiry, owner payouts and bounded portfolio deletion.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PRICE_GAIN: u128 = 150 - 100;
const FACE: [u128; 6] = [2 * PRICE_GAIN, 0, 3 * PRICE_GAIN, 0, 4 * PRICE_GAIN, 0];
const DEPOSITS: [u128; 6] = [1_000, 200, 1_000, 300, 1_000, 50];
const EXPIRY: u64 = 20;

struct World {
    env: V16CuEnv,
    owners: [Keypair; 6],
    portfolios: [Pubkey; 6],
    tokens: [Pubkey; 6],
    provider: Pubkey,
}

impl World {
    fn new(backing: u128) -> Self {
        let deposits = DEPOSITS;
        let params = V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_margin_bps: 1_000,
            maintenance_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        };
        let mut svm = LiteSVM::new();
        let program_id = percolator_prog::id();
        for (id, path) in [
            (program_id, program_path()),
            (spl_token::ID, spl_token_program_path()),
            (
                associated_token_program_id(),
                associated_token_program_path(),
            ),
        ] {
            svm.add_program(id, &std::fs::read(path).unwrap());
        }
        let payer = Keypair::new();
        let admin = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
        let mint = Keypair::new();
        system_create_account_for_test(&mut svm, &payer, &mint, Mint::LEN, spl_token::ID);
        send_raw_tx(
            &mut svm,
            &payer,
            spl_token::instruction::initialize_mint2(
                &spl_token::ID,
                &mint.pubkey(),
                &admin.pubkey(),
                None,
                0,
            )
            .unwrap(),
            &[],
        )
        .unwrap();
        let market = Keypair::new();
        system_create_account_for_test(
            &mut svm,
            &payer,
            &market,
            state::market_account_len_for_capacity(2).unwrap(),
            program_id,
        );
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
        let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
        let init_market_cu = send_tx(
            &mut svm,
            program_id,
            &payer,
            init_market_instruction(&params),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(mint.pubkey(), false),
            ],
            &[&admin],
        )
        .unwrap();
        let mut env = V16CuEnv {
            svm,
            program_id,
            payer,
            admin,
            init_market_cu,
            market: market.pubkey(),
            mint: mint.pubkey(),
            vault,
            vault_authority,
            portfolio_account_len: state::portfolio_account_len_for_market_slots(2).unwrap(),
            portfolios: Vec::new(),
        };
        let owners = std::array::from_fn(|_| Keypair::new());
        let mut portfolios = [Pubkey::default(); 6];
        let mut tokens = portfolios;
        for i in 0..6 {
            env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[&owners[i]],
            )
            .unwrap();
            portfolios[i] = portfolio.pubkey();
            env.portfolios.push(portfolios[i]);
            tokens[i] = create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
            Self::mint(&mut env, tokens[i], deposits[i]);
            env.send(
                env.deposit_ix(portfolios[i], deposits[i]),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap();
        }
        let provider = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        Self::mint(&mut env, provider, backing + 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 1, backing, EXPIRY);
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 3, 1, 12);
        for winner in [0, 2, 4] {
            env.trade_asset_with_cu(
                u16::from(winner == 4),
                &owners[winner],
                portfolios[winner],
                &owners[winner + 1],
                portfolios[winner + 1],
                (FACE[winner] / 50 * POS_SCALE) as i128,
                100,
                0,
            );
        }
        env.configure_permissionless_resolve_with_cu(100, 1);
        for (offset, mark) in (105..=150).step_by(5).enumerate() {
            let slot = 2 + offset as u64;
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(0, slot, mark);
            env.push_auth_mark_for_asset_as_admin(1, slot, mark);
            for actor in [5, 4, 0, 2] {
                env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(u16::from(actor >= 4)),
                    },
                );
            }
        }
        for winner in [0, 2, 4] {
            assert_eq!(
                env.portfolio_state(portfolios[winner]).pnl.get(),
                FACE[winner] as i128
            );
        }
        env.trade_asset_with_cu(
            1,
            &owners[4],
            portfolios[4],
            &owners[5],
            portfolios[5],
            -(FACE[4] as i128 / 50 * POS_SCALE as i128),
            150,
            0,
        );
        for debtor in [1, 3] {
            assert_eq!(
                env.portfolio_state(portfolios[debtor]).capital.get(),
                deposits[debtor]
            );
            assert_eq!(env.portfolio_state(portfolios[debtor]).pnl.get(), 0);
        }
        env.svm.warp_to_slot(12);
        env.resolve();
        env.svm.warp_to_slot(14);
        Self {
            env,
            owners,
            portfolios,
            tokens,
            provider,
        }
    }

    fn mint(env: &mut V16CuEnv, token: Pubkey, amount: u128) {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &env.admin.pubkey(),
                &[],
                amount as u64,
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
    }

    fn payout(&self, actor: usize, topup: bool) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(self.owners[actor].pubkey(), false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: if topup {
                ProgInstruction::ClaimResolvedPayoutTopup
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            }
            .encode(),
        }
    }

    fn land(
        &mut self,
        instructions: &[Instruction],
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        self.env.svm.expire_blockhash();
        let mut all = vec![heap_ix(), cu_ix()];
        all.extend_from_slice(instructions);
        let tx = Transaction::new_signed_with_payer(
            &all,
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer],
            self.env.svm.latest_blockhash(),
        );
        assert_eq!(tx.message.header.num_required_signatures, 1);
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1232);
        self.env.svm.send_transaction(tx)
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        [
            self.env.market,
            self.env.vault,
            self.env.mint,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.provider,
        ]
        .into_iter()
        .chain(self.portfolios)
        .chain(self.tokens)
        .chain(self.owners.iter().map(Signer::pubkey))
        .map(|key| (key, self.env.svm.get_account(&key)))
        .collect()
    }

    fn frame_except(&self, before: &[(Pubkey, Option<Account>)], allowed: &[Pubkey]) {
        for (key, account) in before {
            if !allowed.contains(key) {
                assert_eq!(self.env.svm.get_account(key), *account, "frame {key}");
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct History {
    backing: u128,
    debtors: [usize; 2],
    converter: usize,
    landing: u64,
}

impl History {
    fn waiting(self) -> usize {
        2 - self.converter
    }
    fn denominator(self) -> u128 {
        FACE[4] + FACE[self.waiting()]
    }
    fn pool(self, expired: bool) -> u128 {
        // Both original debtors realize 250 losses in total. Only 250 - backing
        // can fill their source; the remainder joins the unrelated debtor's 50 + 1.
        self.backing + DEPOSITS[5] + 1 + if expired { FACE[self.waiting()] } else { 0 }
    }
    fn junior(self, actor: usize, expired: bool) -> u128 {
        FACE[actor] * self.pool(expired).min(self.denominator()) / self.denominator()
    }
    fn final_paid(self) -> [u128; 6] {
        std::array::from_fn(|i| match i {
            1 | 3 => DEPOSITS[i] - FACE[i - 1],
            5 => 0,
            _ => {
                DEPOSITS[i]
                    + if i == self.converter {
                        FACE[i]
                    } else {
                        self.junior(i, true)
                    }
            }
        })
    }
    fn supply(self) -> u128 {
        DEPOSITS.iter().sum::<u128>() + self.backing + 1
    }

    // Fixed public checkpoints: 0 resolved; 1/2 debtor payouts; 3 flat debt cleared;
    // 4 waiting leg detached; 5 source conversion/payout; 6 unrelated expiry;
    // 7 first receipt; 8 shared expiry; 9 waiting payout; 10 top-up; 11 receipt cleanup.
    // Expectations use deposits, quantities and mark delta, never observed rates.
    fn check(self, world: &World, stage: u8) {
        let group = world.env.market_state().1;
        let portfolios: Vec<_> = world
            .portfolios
            .map(|key| world.env.portfolio_state(key))
            .into();
        // A balanced but misattributed source/residual partition must pass the
        // existing aggregate oracles and fail the independent owner model below.
        crate::support::fuzz_model::assert_market_stock_census(
            "resolved source residual",
            &group,
            &world.env.svm.get_account(&world.env.market).unwrap().data,
            &portfolios,
            u128::from(world.env.token_amount(world.env.vault)),
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "resolved source residual",
            &group,
            &portfolios,
        )
        .unwrap();
        let mut capital = DEPOSITS;
        capital[5] = 0;
        let mut pnl = FACE.map(|v| v as i128);
        pnl[5] = if stage < 3 {
            -(FACE[4] as i128 - DEPOSITS[5] as i128)
        } else {
            0
        };
        let mut paid = [0; 6];
        let mut active = [true, true, true, true, false, false];
        let mut settled_loss = 0;
        for (index, debtor) in self.debtors.into_iter().enumerate() {
            if stage > index as u8 {
                settled_loss += FACE[debtor - 1];
                capital[debtor] = 0;
                paid[debtor] = DEPOSITS[debtor] - FACE[debtor - 1];
                active[debtor] = false;
            }
        }
        let converted = if stage >= 5 { FACE[self.converter] } else { 0 };
        if stage >= 4 {
            active[self.waiting()] = false;
        }
        if stage >= 5 {
            active[self.converter] = false;
            capital[self.converter] = 0;
            pnl[self.converter] = 0;
            paid[self.converter] = DEPOSITS[self.converter] + converted;
        }
        if stage >= 7 {
            capital[4] = 0;
            pnl[4] = 0;
            paid[4] = DEPOSITS[4] + self.junior(4, stage >= 10);
        }
        if stage >= 9 {
            capital[self.waiting()] = 0;
            pnl[self.waiting()] = 0;
            paid[self.waiting()] = DEPOSITS[self.waiting()] + self.junior(self.waiting(), true);
        }
        let fresh = [
            if stage >= 8 {
                0
            } else {
                (self.backing + settled_loss).min(250) - converted
            },
            if stage >= 6 { 0 } else { DEPOSITS[5] + 1 },
        ];
        let source_faces = [
            if stage >= 9 { 0 } else { 250 - converted },
            if stage >= 7 { 0 } else { FACE[4] },
        ];
        let overflow = (self.backing + settled_loss).saturating_sub(250);
        let released = if stage >= 6 { DEPOSITS[5] + 1 } else { 0 }
            + if stage >= 8 { FACE[self.waiting()] } else { 0 };
        let junior_paid = if stage >= 7 { paid[4] - DEPOSITS[4] } else { 0 }
            + if stage >= 9 {
                paid[self.waiting()] - DEPOSITS[self.waiting()]
            } else {
                0
            };
        let vault = self.supply() - paid.iter().sum::<u128>();
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.resolved_slot, 12);
        assert_eq!(group.c_tot, capital.iter().sum::<u128>());
        assert_eq!(
            group.pnl_pos_tot,
            pnl.iter().map(|p| (*p).max(0) as u128).sum::<u128>()
        );
        assert_eq!(
            (group.insurance, group.backing_provider_earnings_total),
            (0, 0)
        );
        assert_eq!(group.vault, vault);
        assert_eq!(u128::from(world.env.token_amount(world.env.vault)), vault);
        assert_eq!(
            vault - group.c_tot - fresh.iter().sum::<u128>(),
            overflow + released - junior_paid,
            "input-derived residual, {self:?}, stage {stage}"
        );
        assert_eq!(
            world
                .tokens
                .map(|key| u128::from(world.env.token_amount(key))),
            paid,
            "owner-local entitlement, {self:?}, stage {stage}"
        );
        assert_eq!(world.env.token_amount(world.provider), 0);
        let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), self.supply());
        assert_eq!(
            group.source_claim_bound_total_num,
            source_faces.iter().sum::<u128>() * BOUND_SCALE
        );
        for (index, domain) in [1, 3].into_iter().enumerate() {
            let source = group.source_credit[domain];
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(
                source.positive_claim_bound_num,
                source_faces[index] * BOUND_SCALE
            );
            assert_eq!(
                source.exact_positive_claim_num,
                source_faces[index] * BOUND_SCALE
            );
            assert_eq!(
                source.fresh_reserved_backing_num,
                fresh[index] * BOUND_SCALE,
                "late debt must fund only the source support gap, stage {stage}"
            );
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                fresh[index] * BOUND_SCALE
            );
            let spent = if index == 0 { converted } else { 0 } * BOUND_SCALE;
            let rate = if source_faces[index] == 0 {
                percolator::CREDIT_RATE_SCALE
            } else {
                (fresh[index] * percolator::CREDIT_RATE_SCALE / source_faces[index])
                    .min(percolator::CREDIT_RATE_SCALE)
            };
            assert_eq!(source.credit_rate_num, rate);
            assert_eq!(
                (
                    source.spent_backing_num,
                    source.provider_receivable_num,
                    bucket.consumed_liened_backing_num
                ),
                (spent, spent, spent)
            );
            assert_eq!(bucket.expiry_slot, if index == 0 { EXPIRY } else { 12 });
            assert_eq!(
                bucket.status,
                if stage >= if index == 0 { 8 } else { 6 } {
                    BackingBucketStatusV16::Expired
                } else {
                    BackingBucketStatusV16::Fresh
                }
            );
            assert_eq!(
                (
                    source.valid_liened_backing_num,
                    source.impaired_liened_backing_num,
                    source.insurance_credit_reserved_num
                ),
                (0, 0, 0)
            );
        }
        if stage >= 7 {
            let ledger = group.resolved_payout_ledger;
            let pool = self.pool(stage >= 8);
            assert_eq!(ledger.snapshot_slot, 14);
            assert_eq!(ledger.snapshot_residual, pool);
            assert_eq!(
                ledger.current_payout_rate_num,
                pool.min(self.denominator()) * BOUND_SCALE
            );
            assert_eq!(
                ledger.current_payout_rate_den,
                self.denominator() * BOUND_SCALE
            );
            assert_eq!(
                ledger.terminal_claim_bound_unreceipted_num,
                if stage >= 9 {
                    0
                } else {
                    FACE[self.waiting()] * BOUND_SCALE
                }
            );
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                if stage >= 9 {
                    self.denominator()
                } else {
                    FACE[4]
                } * BOUND_SCALE
            );
            assert!(!ledger.payout_halted && !ledger.finalized);
        } else {
            assert_eq!(
                group.resolved_payout_ledger,
                ResolvedPayoutLedgerV16::default()
            );
        }
        for (actor, p) in portfolios.iter().enumerate() {
            assert_eq!((p.capital.get(), p.pnl.get()), (capital[actor], pnl[actor]));
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(p)),
                u32::from(active[actor])
            );
            let face = if actor == 4 && stage >= 7 {
                FACE[4]
            } else if actor == self.waiting() && stage >= 9 && self.pool(true) >= self.denominator()
            {
                FACE[actor]
            } else {
                0
            };
            let receipt =
                if face == 0 || (actor == 4 && stage >= 11 && self.junior(4, true) < FACE[4]) {
                    ResolvedPayoutReceiptV16::default()
                } else {
                    let paid_effective = paid[actor] - DEPOSITS[actor];
                    ResolvedPayoutReceiptV16 {
                        present: true,
                        prior_bound_contribution_num: face * BOUND_SCALE,
                        live_released_face_at_receipt: 0,
                        terminal_positive_claim_face: face,
                        paid_effective,
                        finalized: paid_effective == face,
                    }
                };
            assert_eq!(
                resolved_receipt(p),
                receipt,
                "receipt {actor}, stage {stage}"
            );
        }
    }

    fn rank(self, world: &World) -> (u128, u32, u128, u128, u128, usize, u64) {
        let group = world.env.market_state().1;
        let portfolios = world.portfolios.map(|key| world.env.portfolio_state(key));
        // Debt, exposure, fresh backing, source faces, unpaid SPL, nonfinal receipts,
        // then mechanical deletion. Authenticated time advancement is external.
        (
            portfolios[1].capital.get()
                + portfolios[3].capital.get()
                + portfolios[5].pnl.get().unsigned_abs(),
            portfolios
                .iter()
                .map(|p| percolator::active_bitmap_count_ones(active_bitmap(p)))
                .sum(),
            group
                .source_credit
                .iter()
                .map(|s| s.fresh_reserved_backing_num)
                .sum(),
            group.source_claim_bound_total_num,
            self.final_paid().iter().sum::<u128>()
                - world
                    .tokens
                    .map(|t| u128::from(world.env.token_amount(t)))
                    .iter()
                    .sum::<u128>(),
            portfolios
                .iter()
                .filter(|p| {
                    let r = resolved_receipt(p);
                    r.present && !r.finalized
                })
                .count(),
            group.materialized_portfolio_count,
        )
    }
}

#[derive(Default)]
struct Evidence {
    worlds: usize,
    commits: usize,
    rollbacks: usize,
    closes: usize,
    peak: u64,
}

fn step(
    world: &mut World,
    history: History,
    stage: u8,
    actor: usize,
    topup: bool,
    evidence: &mut Evidence,
) {
    let before = world.frame();
    let rank = history.rank(world);
    let vault = world.env.token_amount(world.env.vault);
    let token = world.env.token_amount(world.tokens[actor]);
    let meta = world.land(&[world.payout(actor, topup)]).unwrap();
    assert_cu_within(
        "resolved source residual step",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    evidence.peak = evidence.peak.max(meta.compute_units_consumed);
    evidence.commits += 1;
    assert!(
        history.rank(world) < rank,
        "bounded public progress, stage {stage}"
    );
    assert_eq!(
        vault - world.env.token_amount(world.env.vault),
        world.env.token_amount(world.tokens[actor]) - token
    );
    world.frame_except(
        &before,
        &[
            world.env.market,
            world.env.vault,
            world.portfolios[actor],
            world.tokens[actor],
        ],
    );
    history.check(world, stage);
}

fn rollback(
    world: &mut World,
    history: History,
    stage: u8,
    mut prefix: Vec<Instruction>,
    transfers: usize,
    evidence: &mut Evidence,
) {
    let before = world.frame();
    let mut payer = world
        .env
        .svm
        .get_account(&world.env.payer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature;
    let count = prefix.len();
    prefix.push(Instruction {
        program_id: solana_sdk::system_program::ID,
        accounts: vec![],
        data: vec![],
    });
    let failure = world
        .land(&prefix)
        .expect_err("reject after successful value/classification prefix");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            (count + 2) as u8,
            InstructionError::InvalidInstructionData
        )
    );
    for (program, expected) in [(world.env.program_id, count), (spl_token::ID, transfers)] {
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            expected
        );
    }
    assert_eq!(world.frame(), before);
    assert_eq!(
        world.env.svm.get_account(&world.env.payer.pubkey()),
        Some(payer)
    );
    history.check(world, stage);
    assert_cu_within(
        "resolved source residual rollback",
        failure.meta.compute_units_consumed,
        600_000,
    );
    evidence.peak = evidence.peak.max(failure.meta.compute_units_consumed);
    evidence.rollbacks += 1;
}

#[test]
fn v16_program_late_debtor_source_cap_preserves_receipt_residual_entitlement() {
    let mut evidence = Evidence::default();
    for debtors in [[1, 3], [3, 1]] {
        for offset in [-1i128, 0, 1] {
            for converter in [0, 2] {
                let mut endpoint = None;
                for landing in [EXPIRY, EXPIRY + 1] {
                    let history = History {
                        backing: (250 - FACE[debtors[0] - 1])
                            .checked_add_signed(offset)
                            .unwrap(),
                        debtors,
                        converter,
                        landing,
                    };
                    let mut world = World::new(history.backing);
                    history.check(&world, 0);
                    let identities = world.portfolios.map(|key| {
                        (
                            world.env.portfolio_id(key),
                            world.env.portfolio_position_epoch(key),
                            state::read_portfolio_owner_preflight(
                                &world.env.svm.get_account(&key).unwrap().data,
                            )
                            .unwrap(),
                        )
                    });
                    let first_debt = world.payout(debtors[0], false);
                    rollback(&mut world, history, 0, vec![first_debt], 1, &mut evidence);
                    for (stage, actor) in [
                        (1, debtors[0]),
                        (2, debtors[1]),
                        (3, 5),
                        (4, history.waiting()),
                        (5, converter),
                        (6, 4),
                        (7, 4),
                    ] {
                        step(&mut world, history, stage, actor, false, &mut evidence);
                    }
                    let retained = world.payout(4, true);
                    let original_receipt =
                        resolved_receipt(&world.env.portfolio_state(world.portfolios[4]));
                    assert!(original_receipt.present && !original_receipt.finalized);
                    let before = world.frame();
                    world.land(&[retained.clone()]).unwrap();
                    assert_eq!(
                        world.frame(),
                        before,
                        "zero-due retry must retain future entitlement"
                    );
                    history.check(&world, 7);
                    world.env.svm.warp_to_slot(history.landing);
                    history.check(&world, 7);
                    let waiting = world.payout(history.waiting(), false);
                    rollback(
                        &mut world,
                        history,
                        7,
                        vec![waiting.clone(), waiting, retained.clone()],
                        2,
                        &mut evidence,
                    );
                    step(
                        &mut world,
                        history,
                        8,
                        history.waiting(),
                        false,
                        &mut evidence,
                    );
                    assert_eq!(
                        resolved_receipt(&world.env.portfolio_state(world.portfolios[4])),
                        original_receipt
                    );
                    step(
                        &mut world,
                        history,
                        9,
                        history.waiting(),
                        false,
                        &mut evidence,
                    );
                    step(&mut world, history, 10, 4, true, &mut evidence);
                    if history.junior(4, true) < FACE[4] {
                        step(&mut world, history, 11, 4, true, &mut evidence);
                    }
                    history.check(&world, 11);
                    let before = world.frame();
                    world
                        .land(&[
                            retained.clone(),
                            retained,
                            world.payout(history.waiting(), true),
                        ])
                        .unwrap();
                    assert_eq!(
                        world.frame(),
                        before,
                        "terminal receipt replay cannot repay value"
                    );
                    let paid = world
                        .tokens
                        .map(|key| u128::from(world.env.token_amount(key)));
                    assert_eq!(paid, history.final_paid());
                    let pool = history.pool(true);
                    let surplus = pool.saturating_sub(history.denominator());
                    let rounding = pool.min(history.denominator())
                        - history.junior(4, true)
                        - history.junior(history.waiting(), true);
                    assert_eq!(world.env.market_state().1.vault, surplus + rounding);
                    assert!(rounding <= 1 && surplus <= 2);
                    let result = (
                        paid,
                        world.env.market_state().1.resolved_payout_ledger,
                        surplus,
                        rounding,
                    );
                    assert_eq!(
                        *endpoint.get_or_insert(result),
                        result,
                        "exact/late expiry entitlement"
                    );
                    for actor in 0..6 {
                        let portfolio = world.portfolios[actor];
                        assert_eq!(
                            (
                                world.env.portfolio_id(portfolio),
                                world.env.portfolio_position_epoch(portfolio),
                                state::read_portfolio_owner_preflight(
                                    &world.env.svm.get_account(&portfolio).unwrap().data
                                )
                                .unwrap()
                            ),
                            identities[actor]
                        );
                        assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                        let before = world.frame();
                        let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                        let market_rent = world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports;
                        let count = world.env.market_state().1.materialized_portfolio_count;
                        let cu = world
                            .env
                            .close_portfolio_with_cu(&world.owners[actor], portfolio);
                        assert_cu_within(
                            "source residual portfolio deletion",
                            cu,
                            CUSTODY_CU_LIMIT,
                        );
                        evidence.peak = evidence.peak.max(cu);
                        assert_eq!(
                            world.env.market_state().1.materialized_portfolio_count + 1,
                            count
                        );
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.env.market)
                                .unwrap()
                                .lamports,
                            market_rent + rent
                        );
                        assert!(world
                            .env
                            .svm
                            .get_account(&portfolio)
                            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                        world.frame_except(&before, &[world.env.market, portfolio]);
                        evidence.closes += 1;
                    }
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.materialized_portfolio_count,
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.source_claim_bound_total_num
                        ),
                        (0, 0, 0, 0)
                    );
                    crate::support::fuzz_model::assert_reservation_encumbrance_census(
                        "source residual terminal",
                        &group,
                        &[],
                    )
                    .unwrap();
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (evidence.worlds, evidence.rollbacks, evidence.closes),
        (24, 48, 144)
    );
    println!("resolved source residual: {} worlds, {} ranked commits, {} exact rollbacks, {} portfolio closes, peak {} CU",
        evidence.worlds, evidence.commits, evidence.rollbacks, evidence.closes, evidence.peak);
}
