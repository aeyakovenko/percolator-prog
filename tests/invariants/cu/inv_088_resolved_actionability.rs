//! INV-070/088, row424: a later asset's last blocker releases an earlier waiting claimant.
//! Observe NonProgress before and after partial cleanup, then retry the same public
//! crank after final cleanup without touching the claimant or advancing Clock.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: u64 = 1_000;
const GAIN: u64 = 20;
const SUPPLY: u64 = 4 * CAPITAL;
const LIMIT: u64 = 900_000;
const RESOLVED_SLOT: u64 = 10;
const LANDING_SLOT: u64 = 13;

struct Actor {
    owner: Keypair,
    portfolio: Pubkey,
    token: Pubkey,
}

struct World {
    env: V16CuEnv,
    actors: Vec<Actor>,
    peak: u64,
}

impl World {
    fn new(direction: i128) -> Self {
        use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                max_price_move_bps_per_slot: 10_000,
                ..V16CuMarketParams::default()
            },
        );
        let admin = env.admin.insecure_clone();
        let mut actors = Vec::new();
        for _ in 0..4 {
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
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &admin.pubkey(),
                    &[],
                    CAPITAL,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio.pubkey(), CAPITAL.into()),
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
        env.svm.warp_to_slot(1);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
            let first = 2 * asset as usize;
            env.trade_asset_with_cu(
                asset,
                &actors[first].owner,
                actors[first].portfolio,
                &actors[first + 1].owner,
                actors[first + 1].portfolio,
                direction * POS_SCALE as i128,
                100,
                0,
            );
        }
        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, if direction == 1 { 120 } else { 80 });
        for actor in [2, 0, 1] {
            env.crank(
                actors[actor].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            );
        }
        assert_eq!(
            env.portfolio_state(actors[0].portfolio).pnl.get(),
            GAIN.into()
        );
        env.svm.warp_to_slot(RESOLVED_SLOT);
        env.resolve();
        env.svm.warp_to_slot(LANDING_SLOT);
        Self {
            env,
            actors,
            peak: 0,
        }
    }

    fn crank(&self, actor: usize) -> Instruction {
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
            data: ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: vec![],
            }
            .encode(),
        }
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        let mut keys = vec![
            self.env.market,
            self.env.vault,
            self.env.mint,
            self.env.admin.pubkey(),
        ];
        for actor in &self.actors {
            keys.extend([actor.owner.pubkey(), actor.portfolio, actor.token]);
        }
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    fn assert_frame(&self, before: &[(Pubkey, Option<Account>)], changed: &[Pubkey]) {
        for (key, account) in before {
            if !changed.contains(key) {
                assert_eq!(
                    self.env.svm.get_account(key),
                    *account,
                    "Account frame: {key}"
                );
            }
        }
    }

    fn land(
        &mut self,
        ixs: &[Instruction],
        error: Option<(u8, InstructionError)>,
        successes: (usize, usize),
    ) {
        self.env.svm.expire_blockhash();
        let mut batch = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        ];
        batch.extend_from_slice(ixs);
        let tx = Transaction::new_signed_with_payer(
            &batch,
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer],
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert_eq!(tx.message.header.num_required_signatures, 1);
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let compiled: Vec<_> = tx
            .message
            .account_keys
            .iter()
            .map(|key| (*key, self.env.svm.get_account(key)))
            .collect();
        let before = self.frame();
        let mut payer = self.env.svm.get_account(&self.env.payer.pubkey()).unwrap();
        payer.lamports -= FeeStructure::default().lamports_per_signature;
        let result = self.env.svm.send_transaction(tx);
        let meta = if let Some((index, error)) = error {
            let failure = result.expect_err("waiting or aborted continuation must roll back");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, error)
            );
            self.assert_frame(&before, &[]);
            self.assert_frame(&compiled, &[self.env.payer.pubkey()]);
            failure.meta
        } else {
            result.expect("bounded resolved continuation")
        };
        assert_eq!(
            self.env.svm.get_account(&self.env.payer.pubkey()).unwrap(),
            payer
        );
        for (program, expected) in [
            (self.env.program_id, successes.0),
            (spl_token::ID, successes.1),
        ] {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                expected
            );
        }
        assert_cu_within(
            "row424 resolved actionability",
            meta.compute_units_consumed,
            LIMIT,
        );
        self.peak = self.peak.max(meta.compute_units_consumed);
        assert_eq!(self.env.svm.get_sysvar::<Clock>().slot, LANDING_SLOT);
    }

    fn check(&self, blockers: u64, paid: [u64; 4]) {
        let (_, group) = self.env.market_state();
        let market = self.env.svm.get_account(&self.env.market).unwrap();
        let portfolios: Vec<_> = self
            .actors
            .iter()
            .map(|actor| self.env.portfolio_state(actor.portfolio))
            .collect();
        // In these solvent histories every blocker is one actual stored leg. Count
        // portfolios independently of both the header summary and per-asset totals.
        let stored = portfolios
            .iter()
            .flat_map(|account| &account.legs)
            .filter(|leg| leg.active != 0)
            .count() as u64;
        assert_eq!(stored, blockers);
        assert_eq!(group.resolved_payout_blocker_count, blockers);
        assert_eq!(
            market_group_header_bytes(&market.data)
                .resolved_payout_blocker_count
                .get(),
            blockers
        );
        assert!(group
            .pending_domain_loss_barriers
            .iter()
            .all(|count| *count == 0));
        for asset in &group.assets {
            assert_eq!(
                (
                    asset.stale_account_count_long,
                    asset.stale_account_count_short
                ),
                (0, 0)
            );
        }
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.resolved_slot, RESOLVED_SLOT);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.vault, u128::from(SUPPLY - paid.iter().sum::<u64>()));
        assert_eq!(self.env.token_amount(self.env.vault), group.vault as u64);
        for (actor, expected) in self.actors.iter().zip(paid) {
            assert_eq!(self.env.token_amount(actor.token), expected);
        }
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.freeze_authority, COption::None);
        crate::support::fuzz_model::assert_market_stock_census(
            "row424 resolved actionability",
            &group,
            &market.data,
            &portfolios,
            group.vault,
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "row424 resolved actionability",
            &group,
            &portfolios,
        )
        .unwrap();
    }

    fn waiting(&mut self, retained: &Instruction, blockers: u64, paid: [u64; 4]) {
        let claimant = self.env.portfolio_state(self.actors[0].portfolio);
        assert_eq!(claimant.capital.get(), CAPITAL.into());
        assert_eq!(claimant.pnl.get(), GAIN.into());
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&claimant)));
        assert_eq!((claimant.stale_state, claimant.b_stale_state), (0, 0));
        assert_eq!(claimant.last_fee_slot.get(), RESOLVED_SLOT);
        assert!(!resolved_portfolio_is_terminal(
            &self.env,
            self.actors[0].portfolio
        ));
        self.land(
            &[retained.clone()],
            Some((
                2,
                InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
            )),
            (0, 0),
        );
        self.check(blockers, paid);
    }
}

#[test]
fn v16_program_last_later_asset_blocker_reclassifies_unchanged_earlier_claimant() {
    let mut peak = 0;
    for direction in [1, -1] {
        for order in [[2, 3], [3, 2]] {
            let mut world = World::new(direction);
            let retained = world.crank(0);
            let encoded = retained.data.clone();
            world.check(4, [0; 4]);
            world.land(&[world.crank(1)], None, (1, 1));
            let mut paid = [0, CAPITAL - GAIN, 0, 0];
            world.check(3, paid);
            world.land(&[retained.clone()], None, (1, 0));
            world.waiting(&retained, 2, paid);
            let earlier = world.env.svm.get_account(&world.actors[0].portfolio);

            let first = order[0];
            let before = world.frame();
            world.land(&[world.crank(first)], None, (1, 1));
            world.assert_frame(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.actors[first].portfolio,
                    world.actors[first].token,
                ],
            );
            paid[first] = CAPITAL;
            world.waiting(&retained, 1, paid);
            assert_eq!(
                world.env.svm.get_account(&world.actors[0].portfolio),
                earlier
            );

            let last = order[1];
            let clear_last = world.crank(last);
            // Both the final blocker payout and the newly enabled earlier payout
            // must execute, then an unrelated invalid System suffix rolls all back.
            world.land(
                &[
                    clear_last.clone(),
                    retained.clone(),
                    Instruction {
                        program_id: solana_sdk::system_program::ID,
                        accounts: vec![],
                        data: vec![],
                    },
                ],
                Some((4, InstructionError::InvalidInstructionData)),
                (2, 2),
            );
            world.waiting(&retained, 1, paid);

            let before = world.frame();
            world.land(&[clear_last], None, (1, 1));
            paid[last] = CAPITAL;
            world.assert_frame(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.actors[last].portfolio,
                    world.actors[last].token,
                ],
            );
            world.check(0, paid);
            assert_eq!(
                world.env.svm.get_account(&world.actors[0].portfolio),
                earlier,
                "the later account alone changes the earlier claimant's actionability"
            );
            assert_eq!(retained.data, encoded);

            let before = world.frame();
            world.land(&[retained.clone()], None, (1, 1));
            paid[0] = CAPITAL + GAIN;
            world.assert_frame(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.actors[0].portfolio,
                    world.actors[0].token,
                ],
            );
            world.check(0, paid);
            assert!(world
                .actors
                .iter()
                .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio)));
            let group = world.env.market_state().1;
            assert_eq!((group.c_tot, group.pnl_pos_tot, group.vault), (0, 0, 0));
            world.land(
                &[retained],
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                )),
                (0, 0),
            );
            world.check(0, paid);
            peak = peak.max(world.peak);
            eprintln!("row424 resolved summary: direction={direction}, order={order:?}, blockers=2->1->0, earlier=NonProgress->1020->NonProgress, peak={} CU", world.peak);
        }
    }
    eprintln!("row424: 4 worlds, 4 aborted reclassification/payment prefixes, peak={peak} CU");
}
