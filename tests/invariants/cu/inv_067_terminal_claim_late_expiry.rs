//! Row 417: receipt identity across public SPL recipient invalidation, late stock release,
//! and claimant order. The new boundary is rollback of a successful expiry/payout prefix
//! when a later retained claim rejects, not another engine payout-order proof.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const DEPOSITS: [u128; 5] = [1_000, 500, 1_000, 250, 1_000];
const FACES: [u128; 5] = [14 * 50, 0, 20 * 50, 0, 26 * 50];
const BACKING: u128 = 100;
const EXPIRY: u64 = 13;
// The junior debtor's settled principal is released at slot 12. The backed debtor's
// settled principal joins the provider's reserve and remains senior until slot 13.
const INITIAL_RESIDUAL: u128 = DEPOSITS[1] + 1;
const LATE_RELEASE: u128 = BACKING + DEPOSITS[3];

pub(crate) struct Actor {
    pub(crate) owner: Keypair,
    pub(crate) portfolio: Pubkey,
    pub(crate) token: Pubkey,
}

pub(crate) struct World {
    pub(crate) env: V16CuEnv,
    pub(crate) actors: Vec<Actor>,
    pub(crate) provider_token: Pubkey,
    late_backing_topup: Instruction,
    pub(crate) peak_cu: u64,
}

#[derive(Clone, Copy)]
enum SourceShape {
    Single,
    Staggered,
    SplitClaimants,
}

impl World {
    // INV-066 varies receipt creation across expiry; new() retains INV-067's original seed.
    pub(crate) fn before_receipts() -> Self {
        Self::before_receipts_with_claimant_owners([Keypair::new(), Keypair::new()])
    }

    pub(crate) fn before_receipts_with_claimant_owners(claimant_owners: [Keypair; 2]) -> Self {
        Self::build_before_receipts(claimant_owners, None, BACKING, SourceShape::Single, 0)
    }

    pub(super) fn before_receipts_with_maintenance_fee(rate: u128) -> Self {
        Self::build_before_receipts(
            [Keypair::new(), Keypair::new()],
            None,
            BACKING,
            SourceShape::Single,
            rate,
        )
    }

    pub(super) fn before_receipts_with_backing(backing: u128) -> Self {
        assert!(backing > 0 && backing <= BACKING);
        Self::build_before_receipts(
            [Keypair::new(), Keypair::new()],
            None,
            backing,
            SourceShape::Single,
            0,
        )
    }

    pub(super) fn before_receipts_with_setup(setup: fn(&mut V16CuEnv)) -> Self {
        Self::build_before_receipts(
            [Keypair::new(), Keypair::new()],
            Some(setup),
            BACKING,
            SourceShape::Single,
            0,
        )
    }

    pub(super) fn before_receipts_with_staggered_sources() -> Self {
        Self::build_before_receipts(
            [Keypair::new(), Keypair::new()],
            None,
            BACKING,
            SourceShape::Staggered,
            0,
        )
    }

    pub(super) fn before_receipts_with_split_source_claimants() -> Self {
        Self::build_before_receipts(
            [Keypair::new(), Keypair::new()],
            None,
            BACKING,
            SourceShape::SplitClaimants,
            0,
        )
    }

    fn build_before_receipts(
        claimant_owners: [Keypair; 2],
        setup: Option<fn(&mut V16CuEnv)>,
        backing: u128,
        source_shape: SourceShape,
        maintenance_fee_per_slot: u128,
    ) -> Self {
        // Allocate and initialize through System/SPL/wrapper instructions, including the
        // initial collateral endowment. LiteSVM only supplies programs, clock and signer SOL.
        let staggered_sources = matches!(source_shape, SourceShape::Staggered);
        let split_claimants = matches!(source_shape, SourceShape::SplitClaimants);
        let asset_count = if staggered_sources { 3 } else { 2 };
        let mut deposits = DEPOSITS.to_vec();
        let mut faces = FACES.to_vec();
        let trades = if staggered_sources {
            // Split the same 250 debtor capital, 100 backing and 1,000 claim face
            // across two independent source domains, without changing token supply.
            deposits[3] = 100;
            deposits.push(150);
            faces.push(0);
            vec![(0, 1, 0, 14), (4, 1, 0, 26), (2, 3, 1, 8), (2, 5, 2, 12)]
        } else if split_claimants {
            // Preserve total capital and face, but share one source's fractional rate.
            deposits[2] = 350;
            deposits.push(650);
            faces[2] = 7 * 50;
            faces.push(13 * 50);
            vec![(0, 1, 0, 14), (4, 1, 0, 26), (2, 3, 1, 7), (5, 3, 1, 13)]
        } else {
            vec![(0, 1, 0, 14), (4, 1, 0, 26), (2, 3, 1, 20)]
        };
        let params = V16CuMarketParams {
            maintenance_fee_per_slot,
            max_portfolio_assets: asset_count,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
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
            state::market_account_len_for_capacity(asset_count as usize).unwrap(),
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
            portfolio_account_len: state::portfolio_account_len_for_market_slots(
                asset_count as usize,
            )
            .unwrap(),
            portfolios: Vec::new(),
        };
        if let Some(setup) = setup {
            setup(&mut env);
        }
        let mut actors: Vec<Actor> = Vec::new();
        let mut claimant_owners = claimant_owners.into_iter();
        for (index, deposit) in deposits.into_iter().enumerate() {
            let owner = if index == 0 || index == 4 {
                claimant_owners.next().unwrap()
            } else {
                Keypair::new()
            };
            env.svm.expire_blockhash();
            if actors
                .iter()
                .all(|actor| actor.owner.pubkey() != owner.pubkey())
            {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            }
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[&owner],
            )
            .unwrap();
            env.portfolios.push(portfolio.pubkey());
            let token = actors
                .iter()
                .find(|actor| actor.owner.pubkey() == owner.pubkey())
                .map(|actor| actor.token)
                .unwrap_or_else(|| {
                    create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
                });
            Self::mint(&mut env, token, deposit);
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
            actors.push(Actor {
                owner,
                portfolio: portfolio.pubkey(),
                token,
            });
        }
        let provider_token =
            create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        Self::mint(&mut env, provider_token, BACKING + 2);
        env.svm.warp_to_slot(1);
        for asset in 0..asset_count {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        }
        env.top_up_backing_bucket_from_admin_token_with_cu(provider_token, 1, 1, 12);
        if staggered_sources {
            env.top_up_backing_bucket_from_admin_token_with_cu(provider_token, 3, 61, EXPIRY);
            env.top_up_backing_bucket_from_admin_token_with_cu(provider_token, 5, 39, EXPIRY + 2);
        } else {
            env.top_up_backing_bucket_from_admin_token_with_cu(provider_token, 3, backing, EXPIRY);
        }
        let accounts = vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(provider_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        let mut topup = ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: 3,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount: 1,
            expiry_slot: EXPIRY,
        };
        bind_current_generation_guards(&env.svm, &accounts, &mut topup);
        let late_backing_topup = Instruction {
            program_id,
            accounts,
            data: topup.encode(),
        };
        let mut world = Self {
            env,
            actors,
            provider_token,
            late_backing_topup,
            peak_cu: 0,
        };
        for &(winner, loser, asset, size) in &trades {
            world.trade(winner, loser, asset, size, 100);
        }
        for (offset, mark) in (105..=150).step_by(5).enumerate() {
            let slot = 2 + offset as u64;
            world.env.svm.warp_to_slot(slot);
            for asset in 0..asset_count {
                world
                    .env
                    .push_auth_mark_for_asset_as_admin(asset, slot, mark);
            }
            let actors = if staggered_sources {
                vec![1, 3, 5, 0, 4, 2]
            } else if split_claimants {
                vec![1, 3, 0, 4, 2, 5]
            } else {
                vec![1, 3, 0, 4, 2]
            };
            for actor in actors {
                world.env.crank(
                    world.actors[actor].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(
                            &(0..asset_count).collect::<Vec<_>>(),
                        ),
                    },
                );
            }
            world.custody();
        }
        for &(winner, loser, asset, size) in &trades {
            world.trade(winner, loser, asset, -size, 150);
        }
        for (actor, &face) in faces.iter().enumerate().filter(|(_, face)| **face != 0) {
            assert_eq!(
                world
                    .env
                    .portfolio_state(world.actors[actor].portfolio)
                    .pnl
                    .get(),
                face as i128
            );
        }
        assert_eq!(
            world.env.market_state().1.source_claim_bound_total_num,
            FACES.iter().sum::<u128>() * BOUND_SCALE
        );
        world.env.svm.warp_to_slot(12);
        world.env.resolve();
        let debtors = if staggered_sources {
            vec![1, 3, 5]
        } else {
            vec![1, 3]
        };
        for actor in debtors {
            for _ in 0..8 {
                if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                    break;
                }
                world.land(&[world.payout(actor, false)], false).unwrap();
            }
            assert!(resolved_portfolio_is_terminal(
                &world.env,
                world.actors[actor].portfolio
            ));
        }
        world.custody();
        world
    }

    pub(super) fn new() -> Self {
        let mut world = Self::before_receipts();
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
            assert_eq!(
                receipt.paid_effective,
                Self::entitlement(actor, INITIAL_RESIDUAL)
            );
            assert_eq!(
                receipt.prior_bound_contribution_num,
                FACES[actor] * BOUND_SCALE
            );
            assert_eq!(receipt.live_released_face_at_receipt, 0);
            assert_eq!(
                world.env.token_amount(world.actors[actor].token) as u128,
                DEPOSITS[actor] + receipt.paid_effective
            );
        }
        assert_eq!(
            world
                .env
                .market_state()
                .1
                .resolved_payout_ledger
                .snapshot_residual,
            INITIAL_RESIDUAL
        );
        world.custody();
        world
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

    fn trade(&mut self, winner: usize, loser: usize, asset: u16, size: i128, price: u64) {
        let a = &self.actors[winner];
        let b = &self.actors[loser];
        self.env.svm.expire_blockhash();
        self.env
            .send(
                self.env.trade_no_cpi_ix(
                    a.portfolio,
                    b.portfolio,
                    asset,
                    size * POS_SCALE as i128,
                    price,
                    0,
                ),
                vec![
                    AccountMeta::new(a.owner.pubkey(), true),
                    AccountMeta::new(b.owner.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(a.portfolio, false),
                    AccountMeta::new(b.portfolio, false),
                ],
                &[&a.owner, &b.owner],
            )
            .unwrap();
    }

    pub(crate) fn payout(&self, actor: usize, claim: bool) -> Instruction {
        let actor = &self.actors[actor];
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(actor.owner.pubkey(), false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.token, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: if claim {
                ProgInstruction::ClaimResolvedPayoutTopup
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            }
            .encode(),
        }
    }

    pub(crate) fn land(
        &mut self,
        instructions: &[Instruction],
        admin: bool,
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        self.env.svm.expire_blockhash();
        let mut all = vec![heap_ix(), cu_ix()];
        all.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        if admin {
            signers.push(&self.env.admin);
        }
        let tx = Transaction::new_signed_with_payer(
            &all,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        let result = self.env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(error) => &error.meta,
        };
        self.peak_cu = self.peak_cu.max(meta.compute_units_consumed);
        assert!(meta.compute_units_consumed < 1_400_000);
        result
    }

    pub(crate) fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        // Only the separate network-fee payer and runtime sysvars are excluded.
        let mut keys = vec![
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.provider_token,
        ];
        for actor in &self.actors {
            keys.extend([actor.owner.pubkey(), actor.portfolio, actor.token]);
        }
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    pub(crate) fn custody(&self) {
        let vault = self.env.token_amount(self.env.vault) as u128;
        assert_eq!(self.env.market_state().1.vault, vault);
        let total = vault
            + self.env.token_amount(self.provider_token) as u128
            + self
                .actors
                .iter()
                .map(|actor| actor.token)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|token| self.env.token_amount(token) as u128)
                .sum::<u128>();
        let supply = DEPOSITS.iter().sum::<u128>() + BACKING + 2;
        assert_eq!(total, supply);
        assert_eq!(
            Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            supply
        );
    }

    pub(crate) fn assert_frame_except(
        &self,
        before: &[(Pubkey, Option<Account>)],
        allowed: &[Pubkey],
    ) {
        for (key, account) in before {
            if !allowed.contains(key) {
                assert_eq!(
                    &self.env.svm.get_account(key),
                    account,
                    "unrelated account {key}"
                );
            }
        }
    }

    pub(crate) fn receipt(&self, actor: usize) -> ResolvedPayoutReceiptV16 {
        resolved_receipt(&self.env.portfolio_state(self.actors[actor].portfolio))
    }

    fn entitlement(actor: usize, residual: u128) -> u128 {
        // Small public input integers: independent rational expectation, not an engine call.
        FACES[actor] * residual / FACES.iter().sum::<u128>()
    }

    fn rotate_recipient(&mut self, actor: usize, restore: bool) {
        let before = self.frame();
        let (from, to) = if restore { (1, actor) } else { (actor, 1) };
        self.env.svm.expire_blockhash();
        send_raw_tx(
            &mut self.env.svm,
            &self.env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &self.actors[actor].token,
                Some(&self.actors[to].owner.pubkey()),
                spl_token::instruction::AuthorityType::AccountOwner,
                &self.actors[from].owner.pubkey(),
                &[],
            )
            .unwrap(),
            &[&self.actors[from].owner],
        )
        .unwrap();
        for (key, account) in before {
            if key != self.actors[actor].token {
                assert_eq!(self.env.svm.get_account(&key), account);
            } else {
                let old = TokenAccount::unpack(&account.unwrap().data).unwrap();
                let new =
                    TokenAccount::unpack(&self.env.svm.get_account(&key).unwrap().data).unwrap();
                assert_eq!(new.owner, self.actors[to].owner.pubkey());
                assert_eq!(new.amount, old.amount);
            }
        }
        self.custody();
    }

    fn reject_after_paying_prefix(
        &mut self,
        instructions: &[Instruction],
        admin: bool,
        code: PercolatorError,
    ) {
        let before = self.frame();
        let failure = self
            .land(instructions, admin)
            .expect_err("late instruction must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(4, InstructionError::Custom(code as u32))
        );
        let success = format!("Program {} success", self.env.program_id);
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == success)
                .count(),
            2,
            "expiry and first claim must both succeed before the later rejection"
        );
        assert!(
            failure
                .meta
                .logs
                .iter()
                .any(|line| line == &format!("Program {} success", spl_token::ID)),
            "the rolled-back prefix must include a successful SPL transfer"
        );
        assert_eq!(
            self.frame(),
            before,
            "receipt, stock, SPL and rent must all roll back"
        );
        self.custody();
    }
}

pub(super) fn verify_receipt_payout_and_portfolio_close_retry() {
    // Existing tests reject close after a committed partial payment. Here close rejects
    // in the same transaction, so the positive payment must roll back with the receipt.
    for claim_route in [true, false] {
        let mut world = World::new();
        let claimant = 0;
        let portfolio = world.actors[claimant].portfolio;
        let token = world.actors[claimant].token;
        let retained_payout = world.payout(claimant, claim_route);
        let retained_close = Instruction {
            program_id: world.env.program_id,
            accounts: vec![
                AccountMeta::new(world.actors[claimant].owner.pubkey(), true),
                AccountMeta::new(world.env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: world.env.close_portfolio_ix(portfolio).encode(),
        };
        let original_receipt = world.receipt(claimant);
        world.env.svm.warp_to_slot(EXPIRY);
        world.land(&[world.payout(2, false)], false).unwrap();
        assert_eq!(world.receipt(claimant), original_receipt);
        let expected_paid = World::entitlement(claimant, INITIAL_RESIDUAL + LATE_RELEASE);
        let due = expected_paid
            .checked_sub(original_receipt.paid_effective)
            .unwrap();
        assert!(due > 0 && expected_paid < original_receipt.terminal_positive_claim_face);
        let pending_frame = world.frame();

        for payout_first in [true, false] {
            let instructions = if payout_first {
                [retained_payout.clone(), retained_close.clone()]
            } else {
                [retained_close.clone(), retained_payout.clone()]
            };
            world.env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[
                    heap_ix(),
                    cu_ix(),
                    instructions[0].clone(),
                    instructions[1].clone(),
                ],
                Some(&world.env.payer.pubkey()),
                &[&world.env.payer, &world.actors[claimant].owner],
                world.env.svm.latest_blockhash(),
            );
            let mut expected_payer = world
                .env
                .svm
                .get_account(&world.env.payer.pubkey())
                .unwrap();
            expected_payer.lamports -= solana_sdk::fee::FeeStructure::default()
                .lamports_per_signature
                * u64::from(tx.message.header.num_required_signatures);
            let failure = world
                .env
                .svm
                .send_transaction(tx)
                .expect_err("nonfinal receipt blocks close");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    if payout_first { 3 } else { 2 },
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                ),
            );
            for program in [world.env.program_id, spl_token::ID] {
                let success = format!("Program {program} success");
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == success)
                        .count(),
                    usize::from(payout_first),
                    "payout-first must execute the wrapper and SPL transfer before close rejects",
                );
            }
            assert_cu_within(
                "partial payout/close rollback",
                failure.meta.compute_units_consumed,
                300_000,
            );
            assert_eq!(
                world.frame(),
                pending_frame,
                "receipt, token, rent and peer rollback"
            );
            assert_eq!(
                world
                    .env
                    .svm
                    .get_account(&world.env.payer.pubkey())
                    .unwrap(),
                expected_payer
            );
            world.custody();
        }

        // The other permissionless payout handler must still discharge exactly the same due.
        let alternate_payout = world.payout(claimant, !claim_route);
        let tokens_before = world.env.token_amount(token);
        let vault_before = world.env.token_amount(world.env.vault);
        world.land(&[alternate_payout.clone()], false).unwrap();
        assert_eq!(
            world.env.token_amount(token) as u128,
            tokens_before as u128 + due
        );
        assert_eq!(
            world.env.token_amount(world.env.vault) as u128,
            vault_before as u128 - due
        );
        let mut paid_receipt = original_receipt;
        paid_receipt.paid_effective = expected_paid;
        assert_eq!(
            world.receipt(claimant),
            paid_receipt,
            "only cumulative paid value changes"
        );
        world.assert_frame_except(
            &pending_frame,
            &[world.env.market, world.env.vault, portfolio, token],
        );
        world.custody();
        for replay in [retained_payout, alternate_payout] {
            let before = world.frame();
            match world.land(&[replay], false) {
                Ok(_) => {}
                Err(failure) => assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                    ),
                ),
            }
            assert_eq!(
                world.frame(),
                before,
                "neither payout handler can consume the receipt twice"
            );
            world.custody();
        }

        let expected: [u128; 5] = std::array::from_fn(|actor| {
            if FACES[actor] == 0 {
                0
            } else {
                DEPOSITS[actor] + World::entitlement(actor, INITIAL_RESIDUAL + LATE_RELEASE)
            }
        });
        for _ in 0..16 {
            for actor in [2, 4, 0, 1, 3] {
                if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                    continue;
                }
                let before = world.frame();
                match world.land(&[world.payout(actor, false)], false) {
                    Ok(_) => assert_ne!(world.frame(), before, "remaining close must progress"),
                    Err(failure) => {
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
                world.custody();
                for (actor, limit) in expected.into_iter().enumerate() {
                    assert!(world.env.token_amount(world.actors[actor].token) as u128 <= limit);
                }
            }
            if world
                .actors
                .iter()
                .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio))
            {
                break;
            }
        }
        for (actor, entitlement) in expected.into_iter().enumerate() {
            let portfolio = world.actors[actor].portfolio;
            assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
            assert_eq!(
                world.env.token_amount(world.actors[actor].token) as u128,
                entitlement
            );
            let before = world.frame();
            world.land(&[world.payout(actor, true)], false).unwrap();
            assert_eq!(world.frame(), before, "terminal receipt replay");
            let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
            let market_lamports = world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports;
            world
                .env
                .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
            assert_eq!(
                world
                    .env
                    .svm
                    .get_account(&portfolio)
                    .map_or(0, |account| account.lamports),
                0
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
            world.assert_frame_except(&before, &[world.env.market, portfolio]);
        }
        let group = world.env.market_state().1;
        assert_eq!(group.materialized_portfolio_count, 0);
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.c_tot, 0);
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(group.insurance, 0);
        let rounding_residue = INITIAL_RESIDUAL + LATE_RELEASE
            - [0, 2, 4]
                .into_iter()
                .map(|actor| World::entitlement(actor, INITIAL_RESIDUAL + LATE_RELEASE))
                .sum::<u128>();
        assert_eq!(group.vault, rounding_residue);
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        world.custody();
    }
}

#[test]
fn v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry() {
    let mut peak_cu = 0;
    for landing in [EXPIRY, EXPIRY + 1] {
        for order in [[0, 4], [4, 0]] {
            let mut world = World::new();
            let original = [world.receipt(0), world.receipt(4)];
            let identities = [0, 4].map(|actor| {
                let key = world.actors[actor].portfolio;
                (
                    world.env.portfolio_id(key),
                    world.env.portfolio_position_epoch(key),
                    state::read_portfolio_owner_preflight(
                        &world.env.svm.get_account(&key).unwrap().data,
                    )
                    .unwrap(),
                )
            });
            let snapshot_slot = world
                .env
                .market_state()
                .1
                .resolved_payout_ledger
                .snapshot_slot;
            assert_eq!(snapshot_slot, 12);
            let retained = order.map(|actor| world.payout(actor, true));
            let release = world.payout(2, false);
            world.rotate_recipient(order[1], false);
            world.env.svm.warp_to_slot(landing);
            let before = world.env.market_state().1;
            assert_eq!(
                before.source_backing_buckets[3].status,
                BackingBucketStatusV16::Fresh
            );
            assert_eq!(
                before.source_credit[3].fresh_reserved_backing_num,
                LATE_RELEASE * BOUND_SCALE
            );
            world.reject_after_paying_prefix(
                &[release.clone(), retained[0].clone(), retained[1].clone()],
                false,
                PercolatorError::InvalidTokenAccount,
            );
            assert_eq!([world.receipt(0), world.receipt(4)], original);
            world.rotate_recipient(order[1], true);
            // This retained Live-mode top-up is mode-gated in Resolved. It is not evidence
            // for the separate Live-mode expiry guard.
            world.reject_after_paying_prefix(
                &[
                    release.clone(),
                    retained[0].clone(),
                    world.late_backing_topup.clone(),
                ],
                true,
                PercolatorError::EngineLockActive,
            );
            let release_frame = world.frame();
            world.land(&[release], false).unwrap();
            world.assert_frame_except(
                &release_frame,
                &[world.env.market, world.actors[2].portfolio],
            );
            let after = world.env.market_state().1;
            assert_ne!(
                after.source_backing_buckets[3].status,
                BackingBucketStatusV16::Fresh
            );
            assert_eq!(after.source_credit[3].fresh_reserved_backing_num, 0);
            assert_eq!(
                after.resolved_payout_ledger.snapshot_residual,
                INITIAL_RESIDUAL + LATE_RELEASE
            );
            assert_eq!(after.vault, before.vault);
            assert_eq!([world.receipt(0), world.receipt(4)], original);
            for (index, actor) in order.into_iter().enumerate() {
                let payout_frame = world.frame();
                let vault_before = world.env.token_amount(world.env.vault);
                let tokens_before = world.env.token_amount(world.actors[actor].token);
                let paid_before = world.receipt(actor).paid_effective;
                world.land(&[retained[index].clone()], false).unwrap();
                let receipt = world.receipt(actor);
                let expected = World::entitlement(actor, INITIAL_RESIDUAL + LATE_RELEASE);
                assert!(expected > paid_before);
                assert_eq!(receipt.paid_effective, expected);
                assert_eq!(
                    world.env.token_amount(world.actors[actor].token) - tokens_before,
                    (expected - paid_before) as u64
                );
                assert_eq!(
                    vault_before - world.env.token_amount(world.env.vault),
                    (expected - paid_before) as u64
                );
                world.assert_frame_except(
                    &payout_frame,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        world.actors[actor].token,
                    ],
                );
                for (i, claimant) in [0, 4].into_iter().enumerate() {
                    let mut normalized = world.receipt(claimant);
                    normalized.paid_effective = original[i].paid_effective;
                    assert_eq!(
                        normalized, original[i],
                        "immutable receipt face/prior bound and presence"
                    );
                    let key = world.actors[claimant].portfolio;
                    assert_eq!(
                        (
                            world.env.portfolio_id(key),
                            world.env.portfolio_position_epoch(key),
                            state::read_portfolio_owner_preflight(
                                &world.env.svm.get_account(&key).unwrap().data
                            )
                            .unwrap(),
                        ),
                        identities[i],
                        "market, account, owner and episode provenance"
                    );
                    assert_eq!(
                        world.env.portfolio_state(key).owner,
                        world.actors[claimant].owner.pubkey().to_bytes()
                    );
                }
                let ledger = world.env.market_state().1.resolved_payout_ledger;
                assert_eq!(ledger.snapshot_slot, snapshot_slot);
                assert_eq!(
                    ledger.terminal_claim_exact_receipts_num,
                    (FACES[0] + FACES[4]) * BOUND_SCALE
                );
                assert_eq!(
                    ledger.terminal_claim_bound_unreceipted_num,
                    FACES[2] * BOUND_SCALE
                );
                assert_eq!(
                    ledger.current_payout_rate_num,
                    (INITIAL_RESIDUAL + LATE_RELEASE) * BOUND_SCALE
                );
                assert_eq!(
                    ledger.current_payout_rate_den,
                    FACES.iter().sum::<u128>() * BOUND_SCALE
                );
                world.custody();
                let frame = world.frame();
                world.land(&[retained[index].clone()], false).unwrap();
                assert_eq!(
                    world.frame(),
                    frame,
                    "unchanged retained claim cannot pay twice"
                );
            }
            let expected: [u128; 5] = std::array::from_fn(|actor| {
                if FACES[actor] == 0 {
                    0
                } else {
                    DEPOSITS[actor] + World::entitlement(actor, INITIAL_RESIDUAL + LATE_RELEASE)
                }
            });
            for _ in 0..16 {
                for actor in [2, order[0], order[1], 1, 3] {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        continue;
                    }
                    let frame = world.frame();
                    match world.land(&[world.payout(actor, false)], false) {
                        Ok(_) => {
                            assert_ne!(world.frame(), frame, "terminal continuation must progress")
                        }
                        Err(failure) => {
                            assert_eq!(
                                failure.err,
                                TransactionError::InstructionError(
                                    2,
                                    InstructionError::Custom(
                                        PercolatorError::EngineNonProgress as u32
                                    )
                                )
                            );
                            assert_eq!(world.frame(), frame);
                        }
                    }
                    world.custody();
                    for (i, limit) in expected.into_iter().enumerate() {
                        assert!(world.env.token_amount(world.actors[i].token) as u128 <= limit);
                    }
                }
                if world
                    .actors
                    .iter()
                    .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio))
                {
                    break;
                }
            }
            for (actor, entitlement) in expected.into_iter().enumerate() {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert_eq!(
                    world.env.token_amount(world.actors[actor].token) as u128,
                    entitlement
                );
                let frame = world.frame();
                world.land(&[world.payout(actor, true)], false).unwrap();
                assert_eq!(world.frame(), frame, "terminal claim retry");
                let a = &world.actors[actor];
                let rent = world.env.svm.get_account(&a.portfolio).unwrap().lamports;
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&a.portfolio)
                        .map_or(0, |account| account.lamports),
                    0
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
                world.assert_frame_except(&frame, &[world.env.market, a.portfolio]);
            }
            let group = world.env.market_state().1;
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(group.source_claim_bound_total_num, 0);
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.insurance, 0);
            assert_eq!(group.backing_provider_earnings_total, 0);
            for source in &group.source_credit {
                assert_eq!(source.positive_claim_bound_num, 0);
                assert_eq!(source.fresh_reserved_backing_num, 0);
                assert_eq!(source.valid_liened_backing_num, 0);
                assert_eq!(source.impaired_liened_backing_num, 0);
            }
            let residue = INITIAL_RESIDUAL + LATE_RELEASE
                - [0, 2, 4]
                    .into_iter()
                    .map(|actor| World::entitlement(actor, INITIAL_RESIDUAL + LATE_RELEASE))
                    .sum::<u128>();
            assert_eq!(
                group.vault, residue,
                "only independently computed rounding residue remains"
            );
            assert_eq!(
                world.env.token_amount(world.provider_token),
                1,
                "rejected top-up never debits provider"
            );
            world.custody();
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-067 row 417: 4 public worlds, 8 paid-prefix rollbacks, 8 exact retained top-ups; peak suffix CU {peak_cu}");
}
