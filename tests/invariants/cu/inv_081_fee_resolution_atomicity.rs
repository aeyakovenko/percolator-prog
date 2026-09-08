//! INV-081: an initially executable retained withdrawal crosses a fee/resolution/payout prefix.
//! The late mode rejection must restore the fee cursor, keeper credit, resolution, and SPL payout
//! together. Individually committed retries must satisfy the same independent state/delta oracle.
//! INV-059 owns fee fragmentation and INV-067 owns terminal fee attribution; neither owns this
//! Live -> Resolved -> paid -> rejected Live route -> Live retry composition.
//!
//! The support change exposes only two existing read-only censuses, without changing their bodies.
//! Their existing callers are assert_public_stock_census and assert_public_encumbrance_census,
//! respectively (each scans primary and foreign markets). Those adapters require V16Svm; direct
//! visibility lets this public-System/SPL V16CuEnv scenario reuse the oracles without copying them.
//!
//! Boundary: four flat, solvent, three-owner, single-asset/SPL histories; no first-risk admission,
//! trades, backing expiry, partial receipts, health recertification or CloseSlab. Every owner exits
//! and all insurance is withdrawn, but this is not arbitrary-history closure. Status rows stay open.

use super::super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ENDOWMENTS: [u128; 3] = [1_200, 1_700, 600];
const RATE: u128 = 7;
const SHARE: u16 = 3_333;
const WITHDRAW: u128 = 137;
const KEEPER: usize = 2;

struct Actor {
    owner: Keypair,
    portfolio: Pubkey,
    token: Pubkey,
    id: u64,
}

#[derive(Default)]
struct Expected {
    deposits: [u128; 3],
    fees: [u128; 3],
    rewards: [u128; 3],
    paid: [u128; 3],
    fee_slots: [u64; 3],
    sequences: [u64; 3],
    closed: [bool; 3],
    resolved_slot: Option<u64>,
    insurance_paid: u128,
}

impl Expected {
    fn capital(&self, actor: usize) -> u128 {
        self.deposits[actor] + self.rewards[actor] - self.fees[actor] - self.paid[actor]
    }

    fn collect(&mut self, actor: usize, slot: u64, reward: bool) {
        // The public schedule and configured rate determine liabilities, never observed debits.
        let fee = u128::from(slot - self.fee_slots[actor]) * RATE;
        self.fees[actor] += fee;
        self.fee_slots[actor] = slot;
        if reward {
            self.rewards[KEEPER] += fee * u128::from(SHARE) / 10_000;
        }
    }

    fn insurance(&self) -> u128 {
        self.fees.iter().sum::<u128>() - self.rewards.iter().sum::<u128>() - self.insurance_paid
    }
}

struct World {
    env: V16CuEnv,
    actors: Vec<Actor>,
    admin_token: Pubkey,
    config: state::WrapperConfigV16,
    controls: state::AssetControlSequencesV16,
    expected: Expected,
    successes: usize,
    rejections: usize,
    peak_cu: u64,
}

impl World {
    fn new() -> Self {
        let mut env =
            inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    maintenance_fee_per_slot: RATE,
                    ..V16CuMarketParams::default()
                },
            );
        let mut actors = Vec::new();
        for endowment in ENDOWMENTS {
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
                    &env.admin.pubkey(),
                    &[],
                    u64::try_from(endowment).unwrap(),
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            actors.push(Actor {
                owner,
                portfolio: portfolio.pubkey(),
                token,
                id: env.portfolio_id(portfolio.pubkey()),
            });
        }
        let admin_token =
            create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
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
        .unwrap();
        let config = env.market_state().0;
        let controls = env.control_sequences(0);
        let mut world = Self {
            env,
            actors,
            admin_token,
            config,
            controls,
            expected: Expected::default(),
            successes: 0,
            rejections: 0,
            peak_cu: 0,
        };
        world.check();
        let sequences = world.env.control_sequences(0);
        let policy = world.ix(
            ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps: SHARE,
                policy_sequence: next_control_sequence(sequences.maintenance_fee),
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(world.env.admin.pubkey(), true),
                AccountMeta::new(world.env.market, false),
            ],
        );
        let tx = world.transaction(&[policy], &[], true);
        world.config.maintenance_cranker_fee_share_bps = SHARE;
        world.controls.maintenance_fee = next_control_sequence(sequences.maintenance_fee);
        world.land(tx, &[world.env.market], None);
        for (index, amount) in ENDOWMENTS.into_iter().enumerate() {
            let actor = &world.actors[index];
            let deposit = world.ix(
                world.env.deposit_ix(actor.portfolio, amount),
                vec![
                    AccountMeta::new(actor.owner.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(actor.portfolio, false),
                    AccountMeta::new(actor.token, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let allowed = [
                world.env.market,
                actor.portfolio,
                actor.token,
                world.env.vault,
            ];
            let tx = world.transaction(&[deposit], &[index], false);
            world.expected.deposits[index] = amount;
            world.expected.sequences[index] += 1;
            world.land(tx, &allowed, None);
        }
        world
    }

    fn ix(&self, instruction: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: instruction.encode(),
        }
    }

    fn payout(&self, index: usize, withdraw: Option<u128>) -> Instruction {
        let actor = &self.actors[index];
        self.ix(
            withdraw.map_or(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                |amount| self.env.withdraw_ix(actor.portfolio, amount),
            ),
            vec![
                AccountMeta::new_readonly(actor.owner.pubkey(), withdraw.is_some()),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.token, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }

    fn transaction(
        &self,
        instructions: &[Instruction],
        owners: &[usize],
        admin: bool,
    ) -> Transaction {
        let mut all = vec![heap_ix(), cu_ix()];
        all.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        signers.extend(owners.iter().map(|index| &self.actors[*index].owner));
        if admin {
            signers.push(&self.env.admin);
        }
        Transaction::new_signed_with_payer(
            &all,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        )
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        let mut keys = vec![
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.payer.pubkey(),
            self.env.admin.pubkey(),
            self.admin_token,
        ];
        for actor in &self.actors {
            keys.extend([actor.owner.pubkey(), actor.portfolio, actor.token]);
        }
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    fn land(
        &mut self,
        tx: Transaction,
        allowed: &[Pubkey],
        rejection: Option<(u8, PercolatorError)>,
    ) {
        let fee = u64::from(tx.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let before = self.frame();
        let result = self.env.svm.send_transaction(tx);
        let meta = if let Some((index, code)) = rejection {
            let failure = result.expect_err("retained Live withdrawal must reject in Resolved");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(code as u32))
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| { **line == format!("Program {} success", self.env.program_id) })
                    .count(),
                usize::from(index - 2),
                "every preceding wrapper route must actually succeed"
            );
            if index == 5 {
                assert!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .any(|line| { *line == format!("Program {} success", spl_token::ID) }),
                    "the rolled-back terminal prefix must include a real SPL payout"
                );
            }
            assert!(allowed.is_empty());
            self.rejections += 1;
            failure.meta
        } else {
            self.successes += 1;
            result.unwrap_or_else(|error| {
                panic!("authorized public success #{}: {error:?}", self.successes)
            })
        };
        self.peak_cu = self.peak_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "INV-081 complete route",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        let closed_rent: u64 = self
            .actors
            .iter()
            .enumerate()
            .filter(|(index, _)| self.expected.closed[*index])
            .map(|(_, actor)| {
                before
                    .iter()
                    .find(|(key, _)| *key == actor.portfolio)
                    .unwrap()
                    .1
                    .as_ref()
                    .map_or(0, |account| account.lamports)
            })
            .sum();
        for (key, mut old) in before {
            let after = self.env.svm.get_account(&key);
            if key == self.env.payer.pubkey() {
                old.as_mut().unwrap().lamports -= fee;
            }
            if allowed.contains(&key) {
                if self
                    .actors
                    .iter()
                    .enumerate()
                    .any(|(index, actor)| key == actor.portfolio && self.expected.closed[index])
                {
                    assert!(after
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                    continue;
                }
                let old = old.unwrap();
                let after = after.as_ref().unwrap();
                assert_eq!(
                    after.lamports,
                    old.lamports
                        + if key == self.env.market {
                            closed_rent
                        } else {
                            0
                        }
                );
                assert_eq!(after.owner, old.owner);
                assert_eq!(after.executable, old.executable);
                assert_eq!(after.rent_epoch, old.rent_epoch);
                assert_eq!(after.data.len(), old.data.len());
                if old.owner == spl_token::ID {
                    let mut expected_token = TokenAccount::unpack(&old.data).unwrap();
                    expected_token.amount = TokenAccount::unpack(&after.data).unwrap().amount;
                    let mut expected_bytes = old.data.clone();
                    TokenAccount::pack(expected_token, &mut expected_bytes).unwrap();
                    assert_eq!(
                        after.data, expected_bytes,
                        "SPL amount-only frame for {key}"
                    );
                }
            } else {
                assert_eq!(after, old, "exact persistent frame for {key}");
            }
        }
        self.check();
    }

    fn check(&mut self) {
        let raw_market = self.env.svm.get_account(&self.env.market).unwrap().data;
        let (cfg, group) = state::read_market(&raw_market).unwrap();
        assert_eq!(cfg, self.config, "wrapper authority/policy frame");
        assert_eq!(self.env.control_sequences(0), self.controls);
        let portfolios: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.expected.closed[*index])
            .map(|(_, actor)| self.env.portfolio_state(actor.portfolio))
            .collect();
        let vault = u128::from(self.env.token_amount(self.env.vault));
        assert_market_stock_census("INV-081", &group, &raw_market, &portfolios, vault).unwrap();
        assert_reservation_encumbrance_census("INV-081", &group, &portfolios).unwrap();
        // Production shape checks supplement, but do not supply, the independent raw censuses.
        let mut detached_market = raw_market;
        let (_, view) = state::market_view_mut(&mut detached_market).unwrap();
        view.validate_shape().unwrap();
        assert_eq!(cfg.maintenance_fee_per_slot, RATE);
        assert_eq!(
            group.mode,
            if self.expected.resolved_slot.is_some() {
                MarketModeV16::Resolved
            } else {
                MarketModeV16::Live
            }
        );
        if let Some(slot) = self.expected.resolved_slot {
            assert_eq!(group.resolved_slot, slot);
        }
        for (index, actor) in self.actors.iter().enumerate() {
            let token = TokenAccount::unpack(&self.env.svm.get_account(&actor.token).unwrap().data)
                .unwrap();
            assert_eq!(token.owner, actor.owner.pubkey());
            assert_eq!(token.mint, self.env.mint);
            assert_eq!(
                u128::from(token.amount),
                ENDOWMENTS[index] - self.expected.deposits[index] + self.expected.paid[index]
            );
            if self.expected.closed[index] {
                assert_eq!(self.expected.capital(index), 0);
                assert!(self
                    .env
                    .svm
                    .get_account(&actor.portfolio)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                continue;
            }
            let portfolio = &self.env.portfolio_state(actor.portfolio);
            let mut data = self.env.svm.get_account(&actor.portfolio).unwrap().data;
            state::portfolio_view_mut_for_market_slots(&mut data, 1)
                .unwrap()
                .validate_with_market(&view.as_view())
                .unwrap();
            assert_eq!(portfolio.owner, actor.owner.pubkey().to_bytes());
            assert_eq!(self.env.portfolio_id(actor.portfolio), actor.id);
            assert_eq!(self.env.portfolio_position_epoch(actor.portfolio), 0);
            assert_eq!(
                self.env.portfolio_matcher_sequence(actor.portfolio),
                self.expected.sequences[index]
            );
            assert_eq!(
                portfolio.capital.get(),
                self.expected.capital(index),
                "actor {index} principal/fee/reward/payout attribution"
            );
            assert_eq!(
                portfolio.last_fee_slot.get(),
                self.expected.fee_slots[index]
            );
            assert_eq!(portfolio.pnl.get(), 0);
            assert!(percolator::active_bitmap_is_empty(active_bitmap(portfolio)));
            for raw_leg in &portfolio.legs {
                assert!(
                    !raw_leg.try_to_runtime().unwrap().active,
                    "no hidden position"
                );
            }
            assert!(
                !health_cert(portfolio).valid,
                "flat value routes must not manufacture a certificate"
            );
        }
        for asset in &group.assets {
            assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (0, 0));
            assert_eq!(
                (asset.stored_pos_count_long, asset.stored_pos_count_short),
                (0, 0)
            );
        }
        assert_eq!(group.insurance, self.expected.insurance());
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            self.expected.insurance()
        );
        assert_eq!(group.vault, group.c_tot + self.expected.insurance());
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        let supply = ENDOWMENTS.iter().sum::<u128>();
        assert_eq!(u128::from(mint.supply), supply);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.freeze_authority, COption::None);
        let spl_vault =
            TokenAccount::unpack(&self.env.svm.get_account(&self.env.vault).unwrap().data).unwrap();
        assert_eq!(spl_vault.owner, self.env.vault_authority);
        assert_eq!(spl_vault.mint, self.env.mint);
        assert_eq!(spl_vault.delegate, COption::None);
        assert_eq!(spl_vault.close_authority, COption::None);
        assert_eq!(
            self.env.vault,
            canonical_vault_ata(self.env.vault_authority, self.env.mint)
        );
        assert_eq!(
            u128::from(self.env.token_amount(self.admin_token)),
            self.expected.insurance_paid
        );
        assert_eq!(
            vault
                + u128::from(self.env.token_amount(self.admin_token))
                + self
                    .actors
                    .iter()
                    .map(|actor| u128::from(self.env.token_amount(actor.token)))
                    .sum::<u128>(),
            supply
        );
    }
}

#[test]
fn v16_program_retained_withdrawal_rolls_back_fee_resolution_and_paid_prefix() {
    for slot in [11, 29] {
        for payer in [0, 1] {
            let mut world = World::new();
            let other = 1 - payer;
            let retained_ix = world.payout(payer, Some(WITHDRAW));
            let retained = world.transaction(&[retained_ix.clone()], &[payer], false);
            let before = world.frame();
            world
                .env
                .svm
                .simulate_transaction(retained.clone().into())
                .expect("the exact signed retained withdrawal is initially executable");
            assert_eq!(world.frame(), before);
            world.env.svm.warp_to_slot(slot);
            let sync = world.ix(
                ProgInstruction::SyncMaintenanceFee { now_slot: slot },
                vec![
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.actors[payer].portfolio, false),
                    AccountMeta::new(world.actors[KEEPER].portfolio, false),
                ],
            );
            let resolve = world.ix(
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: world.env.market_state().1.next_market_id,
                    authority_epoch: world.env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(world.env.admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                ],
            );
            let terminal_payout = world.payout(other, None);
            let abort = world.transaction(
                &[
                    sync.clone(),
                    resolve.clone(),
                    terminal_payout.clone(),
                    retained_ix,
                ],
                &[payer],
                true,
            );
            world.land(abort, &[], Some((5, PercolatorError::EngineLockActive)));
            assert_eq!(world.expected.fees, [0; 3]);

            let tx = world.transaction(&[sync], &[], false);
            world.expected.collect(payer, slot, true);
            world.land(
                tx,
                &[
                    world.env.market,
                    world.actors[payer].portfolio,
                    world.actors[KEEPER].portfolio,
                ],
                None,
            );
            assert!(world.expected.rewards[KEEPER] > 0);
            world.expected.paid[payer] += WITHDRAW;
            world.expected.sequences[payer] += 1;
            // Deliver the original signed transaction, without rebinding or re-signing it.
            world.land(
                retained,
                &[
                    world.env.market,
                    world.actors[payer].portfolio,
                    world.actors[payer].token,
                    world.env.vault,
                ],
                None,
            );

            let mode_probe = world.transaction(&[world.payout(payer, Some(1))], &[payer], false);
            let tx = world.transaction(&[resolve], &[], true);
            world.expected.resolved_slot = Some(slot);
            world.land(tx, &[world.env.market], None);
            world.land(
                mode_probe,
                &[],
                Some((2, PercolatorError::EngineLockActive)),
            );

            // Only terminal fees remain unsynchronized for the other owner and the keeper.
            // Delayed public payouts charge through resolution, not through the later clock.
            world.env.svm.warp_to_slot(slot + 9);
            for index in [other, payer, KEEPER] {
                let tx = world.transaction(&[world.payout(index, None)], &[], false);
                world.expected.collect(index, slot, false);
                world.expected.paid[index] += world.expected.capital(index);
                world.land(
                    tx,
                    &[
                        world.env.market,
                        world.actors[index].portfolio,
                        world.actors[index].token,
                        world.env.vault,
                    ],
                    None,
                );
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[index].portfolio
                ));
            }
            for index in [other, payer, KEEPER] {
                let actor = &world.actors[index];
                let close = world.ix(
                    world.env.close_portfolio_ix(actor.portfolio),
                    vec![
                        AccountMeta::new(actor.owner.pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(actor.portfolio, false),
                    ],
                );
                let tx = world.transaction(&[close], &[index], false);
                world.expected.closed[index] = true;
                world.land(tx, &[world.env.market, world.actors[index].portfolio], None);
            }
            let amount = world.expected.insurance();
            let insurance = world.ix(
                world
                    .env
                    .withdraw_insurance_asset_instruction(world.env.admin.pubkey(), 0, amount),
                vec![
                    AccountMeta::new(world.env.admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.admin_token, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let tx = world.transaction(&[insurance], &[], true);
            world.expected.insurance_paid = amount;
            world.land(
                tx,
                &[world.env.market, world.admin_token, world.env.vault],
                None,
            );
            assert_eq!(world.env.token_amount(world.env.vault), 0);
            assert_eq!(world.rejections, 2);
            assert_eq!(world.successes, 14);
            println!("INV-081 slot={slot} payer={payer}: {} checked successes, {} exact rollbacks, peak={} CU",
                world.successes, world.rejections, world.peak_cu);
        }
    }
}
