//! Finding-blind INV-057 coverage: bounded owner witnesses from funded lifecycle states.
//! Economic genesis uses public System/SPL/ATA/wrapper instructions. Exit transactions
//! have exactly one signer, the exposed owner, who also pays the transaction fee.
//! These finite histories are witnesses, not a universal liveness proof.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const PRINCIPAL: u128 = 1_000;
const IDLE_PRINCIPAL: u128 = 137;
const OPEN_Q: u128 = 2 * POS_SCALE;
const TERMINAL_STEPS: usize = 8;

struct FundedOwners {
    env: V16CuEnv,
    owners: Vec<Keypair>,
    portfolios: Vec<Pubkey>,
    tokens: Vec<Pubkey>,
    supply: u128,
}

impl FundedOwners {
    fn new(capital: &[u128], close_chunk: u128) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                max_bankrupt_close_lifetime_slots: 2,
                public_b_chunk_atoms: close_chunk,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
        }
        env.configure_permissionless_resolve_with_cu(100, 5);
        let mut owners = Vec::new();
        let mut portfolios = Vec::new();
        let mut tokens = Vec::new();
        for &amount in capital {
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
            .expect("initialize System-created portfolio");
            env.portfolios.push(portfolio.pubkey());
            let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    u64::try_from(amount).unwrap(),
                )
                .unwrap(),
                &[&env.admin],
            )
            .expect("mint finite user supply");
            env.send(
                env.deposit_ix(portfolio.pubkey(), amount),
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
            .expect("fund portfolio by SPL transfer");
            owners.push(owner);
            portfolios.push(portfolio.pubkey());
            tokens.push(token);
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .expect("seal the funded supply");
        let funded = Self {
            env,
            owners,
            portfolios,
            tokens,
            supply: capital.iter().sum(),
        };
        funded.assert_senior(capital);
        funded
    }

    fn paired(asset: u16, sign: i128) -> Self {
        let mut funded = Self::new(&[PRINCIPAL, PRINCIPAL, IDLE_PRINCIPAL], 1_000);
        funded.open(0, 1, asset, sign * OPEN_Q as i128);
        funded.open(0, 1, 1 - asset, -sign * POS_SCALE as i128);
        assert_eq!(funded.quantity(0, asset), OPEN_Q);
        assert_eq!(funded.quantity(0, 1 - asset), POS_SCALE);
        funded
    }

    fn open(&mut self, first: usize, second: usize, asset: u16, size: i128) {
        let cu = self.env.trade_asset_with_cu(
            asset,
            &self.owners[first],
            self.portfolios[first],
            &self.owners[second],
            self.portfolios[second],
            size,
            100,
            0,
        );
        assert_cu_within("INV-057 funded open", cu, MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
    }

    fn account(&self, actor: usize) -> PortfolioAccountV16 {
        self.env.portfolio_state(self.portfolios[actor])
    }

    fn quantity(&self, actor: usize, asset: u16) -> u128 {
        let account = self.account(actor);
        if !has_active_leg_for_asset(&account, asset as usize) {
            return 0;
        }
        reference_current_epoch_effective_abs(
            &self.env.market_state().1,
            active_leg_for_asset(&account, asset as usize),
        )
    }

    fn stored_quantity(&self, actor: usize) -> u128 {
        let account = self.account(actor);
        (0..2)
            .filter(|&asset| has_active_leg_for_asset(&account, asset))
            .map(|asset| {
                active_leg_for_asset(&account, asset)
                    .basis_pos_q
                    .unsigned_abs()
            })
            .sum()
    }

    fn frame(&self, actor: usize, payout: bool) -> Vec<(Pubkey, Account)> {
        let mut keys = vec![
            self.env.admin.pubkey(),
            self.env.payer.pubkey(),
            self.env.mint,
        ];
        if !payout {
            keys.push(self.env.vault);
        }
        for index in 0..self.owners.len() {
            if index != actor {
                keys.extend([self.owners[index].pubkey(), self.portfolios[index]]);
            }
            if !payout || index != actor {
                keys.push(self.tokens[index]);
            }
        }
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key).unwrap()))
            .collect()
    }

    fn assert_frame(&self, frame: Vec<(Pubkey, Account)>) {
        for (key, before) in frame {
            assert_eq!(
                self.env.svm.get_account(&key).unwrap(),
                before,
                "frame {key}"
            );
        }
    }

    fn assert_senior(&self, rights: &[u128]) {
        assert_eq!(rights.len(), self.owners.len());
        let group = self.env.market_state().1;
        let mut capital = 0;
        let mut paid = 0;
        for (actor, &right) in rights.iter().enumerate() {
            let account = self.account(actor);
            let tokens = u128::from(self.env.token_amount(self.tokens[actor]));
            let junior_paid = resolved_receipt(&account).paid_effective;
            assert_eq!(
                account.capital.get() + tokens,
                right + junior_paid,
                "owner {actor}: principal cannot fund another owner's exit"
            );
            capital += account.capital.get();
            paid += tokens;
        }
        assert_eq!(group.c_tot, capital);
        assert_eq!(
            group.insurance, 0,
            "these zero-fee histories do not need an insurance subsidy"
        );
        assert!(group.vault >= group.c_tot + group.insurance);
        assert_eq!(
            group.vault,
            u128::from(self.env.token_amount(self.env.vault))
        );
        assert_eq!(group.vault + paid, self.supply);
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), self.supply);
        assert_eq!(mint.mint_authority, COption::None);
    }

    fn call(&mut self, actor: usize, ix: ProgInstruction, payout: bool, label: &str) -> u64 {
        let mut accounts = vec![
            AccountMeta::new(self.owners[actor].pubkey(), true),
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.portfolios[actor], false),
        ];
        if payout {
            accounts.extend([
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ]);
        }
        let frame = self.frame(actor, payout);
        let mut payer_before = self
            .env
            .svm
            .get_account(&self.owners[actor].pubkey())
            .unwrap();
        self.env.svm.expire_blockhash();
        let cu = send_tx(
            &mut self.env.svm,
            self.env.program_id,
            &self.owners[actor],
            ix,
            accounts,
            &[],
        )
        .unwrap_or_else(|error| panic!("{label}: owner-only public witness failed: {error}"));
        assert_cu_within(label, cu, CRANK_CU_LIMIT);
        payer_before.lamports -= solana_sdk::fee::FeeStructure::default().lamports_per_signature;
        assert_eq!(
            self.env
                .svm
                .get_account(&self.owners[actor].pubkey())
                .unwrap(),
            payer_before
        );
        self.assert_frame(frame);
        cu
    }

    fn reduce(&mut self, actor: usize, asset: u16, quantity: u128) -> u64 {
        self.call(
            actor,
            ProgInstruction::RebalanceReduce {
                portfolio_id: self.env.portfolio_id(self.portfolios[actor]),
                position_epoch: self.env.portfolio_position_epoch(self.portfolios[actor]),
                asset_index: asset,
                reduce_q: quantity,
            },
            false,
            "INV-057 owner reduction",
        )
    }

    fn forfeit(&mut self, actor: usize, asset: u16, budget: u128) -> u64 {
        self.call(
            actor,
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id: self.env.portfolio_id(self.portfolios[actor]),
                position_epoch: self.env.portfolio_position_epoch(self.portfolios[actor]),
                asset_index: asset,
                b_delta_budget: budget,
            },
            false,
            "INV-057 owner recovery forfeiture",
        )
    }

    fn crank(&mut self, actor: usize, observations: Vec<CrankObservationHint>) -> u64 {
        self.call(
            actor,
            ProgInstruction::PermissionlessCrank {
                now_slot: self.env.svm.get_sysvar::<Clock>().slot,
                observations,
            },
            false,
            "INV-057 owner crank",
        )
    }

    fn terminal_witness(&mut self, actor: usize, rights: &[u128]) -> (usize, u64) {
        let quantity_before = self.stored_quantity(actor);
        assert!(quantity_before > 0, "terminal route starts exposed");
        assert_eq!(
            quantity_before,
            self.quantity(actor, 0) + self.quantity(actor, 1),
            "the starting quantity is live risk, not zero-effective reset residue"
        );
        assert!(!resolved_receipt(&self.account(actor)).present);
        let mut max_cu = 0;
        for step in 1..=TERMINAL_STEPS {
            let before = self.account(actor);
            let cu = self.call(
                actor,
                ProgInstruction::PermissionlessCrank {
                    now_slot: self.env.svm.get_sysvar::<Clock>().slot,
                    observations: vec![],
                },
                true,
                "INV-057 owner terminal continuation",
            );
            max_cu = max_cu.max(cu);
            self.assert_senior(rights);
            let after = self.account(actor);
            assert_ne!(
                after, before,
                "terminal continuation must do account-local work"
            );
            if self.stored_quantity(actor) < quantity_before || resolved_receipt(&after).present {
                assert!(
                    self.quantity(actor, 0) + self.quantity(actor, 1) < quantity_before
                        || resolved_receipt(&after).present
                );
                return (step, max_cu);
            }
        }
        panic!("owner did not reduce exposure or create a receipt in {TERMINAL_STEPS} calls");
    }
}

#[test]
fn v16_program_funded_live_and_drain_owner_reduction_matrix() {
    for asset in 0..2 {
        for sign in [-1, 1] {
            for drain in [false, true] {
                let mut f = FundedOwners::paired(asset, sign);
                if drain {
                    f.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_DRAIN_ONLY,
                        asset,
                        0,
                        0,
                    );
                }
                let before = f.env.market_state().1;
                assert_eq!(before.mode, MarketModeV16::Live);
                assert_eq!(
                    before.assets[asset as usize].lifecycle,
                    if drain {
                        AssetLifecycleV16::DrainOnly
                    } else {
                        AssetLifecycleV16::Active
                    }
                );
                assert_eq!(f.quantity(0, asset), OPEN_Q);
                let epoch = f.env.portfolio_position_epoch(f.portfolios[0]);
                let cu = f.reduce(0, asset, POS_SCALE);
                let after = f.env.market_state().1;
                assert_eq!(f.quantity(0, asset), OPEN_Q - POS_SCALE);
                assert_eq!(f.quantity(1, asset), OPEN_Q - POS_SCALE);
                assert_eq!(f.quantity(0, 1 - asset), POS_SCALE);
                assert_eq!(
                    after.assets[(1 - asset) as usize],
                    before.assets[(1 - asset) as usize]
                );
                assert_eq!(
                    after.assets[asset as usize].lifecycle,
                    before.assets[asset as usize].lifecycle
                );
                assert_eq!(
                    after.assets[asset as usize].oi_eff_long_q,
                    OPEN_Q - POS_SCALE
                );
                assert_eq!(
                    after.assets[asset as usize].oi_eff_short_q,
                    OPEN_Q - POS_SCALE
                );
                assert_eq!(f.env.portfolio_position_epoch(f.portfolios[0]), epoch + 1);
                assert_eq!(f.account(0).pnl.get(), 0);
                f.assert_senior(&[PRINCIPAL, PRINCIPAL, IDLE_PRINCIPAL]);
                eprintln!(
                    "INV-057 live/drain asset={asset} sign={sign} drain={drain}: 1 call, {cu} CU"
                );
            }
        }
    }
}

#[test]
fn v16_program_funded_reset_with_live_sibling_has_owner_exit() {
    for asset in 0..2 {
        for sign in [-1, 1] {
            let mut f = FundedOwners::paired(asset, sign);
            f.reduce(0, asset, OPEN_Q);
            let reset = f.env.market_state().1.assets[asset as usize];
            let (mode, stored) = if sign > 0 {
                (reset.mode_short, reset.stored_pos_count_short)
            } else {
                (reset.mode_long, reset.stored_pos_count_long)
            };
            assert_eq!(mode, SideModeV16::ResetPending);
            assert_eq!(stored, 1);
            assert!(has_active_leg_for_asset(&f.account(1), asset as usize));
            assert_eq!(
                f.quantity(1, 1 - asset),
                POS_SCALE,
                "reset owner still has live risk"
            );
            let cleanup = f.crank(1, vec![]);
            assert!(!has_active_leg_for_asset(&f.account(1), asset as usize));
            f.assert_senior(&[PRINCIPAL, PRINCIPAL, IDLE_PRINCIPAL]);
            let reduction = f.reduce(1, 1 - asset, POS_SCALE / 2);
            assert_eq!(f.quantity(1, 1 - asset), POS_SCALE / 2);
            assert_eq!(f.quantity(0, 1 - asset), POS_SCALE / 2);
            f.assert_senior(&[PRINCIPAL, PRINCIPAL, IDLE_PRINCIPAL]);
            eprintln!(
                "INV-057 reset asset={asset} sign={sign}: 2 calls, CU=[{cleanup},{reduction}]"
            );
        }
    }
}

#[test]
fn v16_program_funded_recovery_forfeits_only_unbooked_junior_gain() {
    for asset in 0..2 {
        for sign in [-1, 1] {
            let mut f = FundedOwners::paired(asset, sign);
            let mark: u64 = if sign > 0 { 105 } else { 95 };
            let gain = OPEN_Q / POS_SCALE * u128::from(mark.abs_diff(100));
            f.env.svm.warp_to_slot(2);
            f.env.push_auth_mark_for_asset_as_admin(asset, 2, mark);
            f.crank(1, crank_observations_for_assets(&[asset, 1 - asset]));
            assert_eq!(
                f.env.market_state().1.assets[asset as usize].effective_price,
                mark
            );
            assert_eq!(f.account(1).capital.get(), PRINCIPAL - gain);
            assert_eq!(f.account(0).pnl.get(), 0);
            let unrefreshed = active_leg_for_asset(&f.account(0), asset as usize);
            let moved = f.env.market_state().1.assets[asset as usize];
            assert_ne!(
                unrefreshed.k_snap,
                if sign > 0 {
                    moved.k_long
                } else {
                    moved.k_short
                }
            );
            assert!(gain > 0);
            f.env.update_asset_lifecycle_as_admin_with_cu(
                processor::ASSET_ACTION_SHUTDOWN,
                asset,
                2,
                0,
            );
            let before = f.env.market_state().1;
            assert_eq!(before.mode, MarketModeV16::Live);
            assert_eq!(
                before.assets[asset as usize].lifecycle,
                AssetLifecycleV16::Recovery
            );
            assert_eq!(f.quantity(0, asset), OPEN_Q);
            let cu = f.forfeit(0, asset, 1_000);
            assert_eq!(f.quantity(0, asset), 0);
            assert_eq!(f.quantity(0, 1 - asset), POS_SCALE);
            assert_eq!(
                f.account(0).pnl.get(),
                0,
                "no conversion of the surrendered gain"
            );
            assert_eq!(f.account(0).reserved_pnl.get(), 0);
            assert_eq!(
                f.env.market_state().1.assets[(1 - asset) as usize],
                before.assets[(1 - asset) as usize]
            );
            f.assert_senior(&[PRINCIPAL, PRINCIPAL - gain, IDLE_PRINCIPAL]);
            eprintln!("INV-057 recovery asset={asset} sign={sign}: junior={gain}, 1 call, {cu} CU");
        }
    }
}

#[test]
fn v16_program_funded_resolved_owner_reduction_inside_owner_window() {
    for asset in 0..2 {
        for sign in [-1, 1] {
            let mut f = FundedOwners::paired(asset, sign);
            let mark = if sign > 0 { 105 } else { 95 };
            let gain = 10;
            f.env.svm.warp_to_slot(2);
            f.env.push_auth_mark_for_asset_as_admin(asset, 2, mark);
            f.crank(1, crank_observations_for_assets(&[asset, 1 - asset]));
            f.env.resolve();
            let resolved = f.env.market_state().1;
            assert_eq!(resolved.mode, MarketModeV16::Resolved);
            assert_eq!(resolved.resolved_slot, f.env.svm.get_sysvar::<Clock>().slot);
            assert_eq!(f.env.market_state().0.force_close_delay_slots, 5);
            assert_eq!(f.stored_quantity(0), OPEN_Q + POS_SCALE);
            let rights = [PRINCIPAL, PRINCIPAL - gain, IDLE_PRINCIPAL];
            let (steps, cu) = f.terminal_witness(0, &rights);
            assert!(f.stored_quantity(0) < OPEN_Q + POS_SCALE);
            assert_eq!(
                f.env.svm.get_sysvar::<Clock>().slot,
                resolved.resolved_slot,
                "no wait for the permissionless window or another actor"
            );
            eprintln!("INV-057 resolved asset={asset} sign={sign}: {steps} calls, max {cu} CU");
        }
    }
}

#[test]
fn v16_program_funded_active_and_expired_close_keep_owner_terminal_route() {
    for expired in [false, true] {
        let rights = [0, PRINCIPAL, IDLE_PRINCIPAL, PRINCIPAL];
        let mut f = FundedOwners::new(&[2, PRINCIPAL, IDLE_PRINCIPAL, PRINCIPAL], 1);
        f.open(1, 3, 0, POS_SCALE as i128);
        f.open(1, 0, 1, (POS_SCALE / 50) as i128);
        for (slot, mark) in [(2, 200), (3, 300)] {
            f.env.svm.warp_to_slot(slot);
            f.env.push_auth_mark_for_asset_as_admin(1, slot, mark);
            f.crank(1, crank_observations_for_assets(&[1, 0]));
        }
        f.crank(0, crank_observations(1));
        f.env.svm.warp_to_slot(4);
        f.env
            .update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 1, 4, 0);
        f.forfeit(0, 1, 1);
        let ledger = close_progress(&f.account(0));
        assert!(ledger.active && !ledger.finalized && ledger.residual_remaining > 0);
        assert_eq!(ledger.asset_index, 1);
        assert_eq!(f.env.market_state().1.mode, MarketModeV16::Live);
        let quantity_before = f.stored_quantity(0);
        assert!(quantity_before > 0);
        f.assert_senior(&rights);
        let mut calls = 0;
        let mut max_cu = 0;
        if expired {
            f.env.svm.warp_to_slot(ledger.max_close_slot + 1);
            max_cu = max_cu.max(f.crank(0, vec![]));
            calls += 1;
            assert_eq!(f.env.market_state().1.mode, MarketModeV16::Recovery);
            assert_eq!(
                close_progress(&f.account(0)),
                ledger,
                "preemption preserves the close obligation until terminal settlement"
            );
            f.assert_senior(&rights);
            max_cu = max_cu.max(f.crank(0, vec![]));
            calls += 1;
            assert_eq!(f.env.market_state().1.mode, MarketModeV16::Resolved);
            f.assert_senior(&rights);
            let (steps, cu) = f.terminal_witness(0, &rights);
            calls += steps;
            max_cu = max_cu.max(cu);
            assert!(f.stored_quantity(0) < quantity_before);
        } else {
            for _ in 0..2 {
                let before = close_progress(&f.account(0));
                max_cu = max_cu.max(f.crank(0, vec![]));
                calls += 1;
                let after = close_progress(&f.account(0));
                assert_eq!(after.close_id, ledger.close_id);
                assert!(
                    after.residual_remaining < before.residual_remaining
                        || (!before.finalized && after.finalized),
                    "close step must discharge or finalize debt: {before:?} -> {after:?}"
                );
                f.assert_senior(&rights);
                if after.finalized {
                    break;
                }
            }
            assert!(close_progress(&f.account(0)).finalized);
            max_cu = max_cu.max(f.forfeit(0, 1, 1_000));
            calls += 1;
            assert!(
                f.stored_quantity(0) < quantity_before,
                "close continuation must reduce exposure, not merely change a cursor"
            );
        }
        f.assert_senior(&rights);
        assert!(calls <= TERMINAL_STEPS + 2);
        eprintln!("INV-057 close expired={expired}: {calls} calls, max {max_cu} CU");
    }
}
