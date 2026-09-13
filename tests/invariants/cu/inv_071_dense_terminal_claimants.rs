//! Scope K: dense exposed claimants and reserve disposition at supported capacity.

use super::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const LEGS: usize = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
const CAPITAL: u64 = 100_000;
const START: u64 = 10_000;
const END: u64 = START + 4;
const EXPIRY: u64 = END + 100;
const LIMIT: u64 = 1_375_000;

struct Exposure {
    asset: u16,
    units: u64,
    long: bool,
    backing: u64,
    insurance: u64,
}

impl Exposure {
    fn domain(&self) -> usize {
        2 * usize::from(self.asset) + usize::from(self.long)
    }
}

struct DenseWorld {
    env: V16CuEnv,
    exposures: Vec<Exposure>,
    owners: [Pubkey; 4],
    portfolios: [Pubkey; 4],
    tokens: [Pubkey; 4],
    provider: Pubkey,
    insurer: Pubkey,
    provider_token: Pubkey,
    insurer_token: Pubkey,
    destination: Pubkey,
    entitlement: [u64; 4],
    supply: u64,
    tracked: Vec<Pubkey>,
    steps: usize,
    rollbacks: usize,
    rollback_payouts: usize,
    peak: u64,
}

impl DenseWorld {
    fn new(seed: u64) -> Self {
        use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;
        assert_eq!(LEGS, 14);
        let mut rng = StdRng::seed_from_u64(seed);
        let mut assets = vec![0, 1, 13, 14, 254, 255, 256, 257, 510, 511, 512, 513];
        assets.extend((MAX_10M_MARKET_SLOTS - 16)..MAX_10M_MARKET_SLOTS);
        assert_eq!(assets.len(), 2 * LEGS);
        let exposures: Vec<_> = assets
            .into_iter()
            .enumerate()
            .map(|(i, asset)| Exposure {
                asset: asset as u16,
                units: rng.gen_range(1..=5),
                long: (i + seed as usize) % 2 == 0,
                backing: rng.gen_range(31..=71),
                insurance: rng.gen_range(11..=29),
            })
            .collect();
        let mut env = inv018_public_spl_market_with_capacity(
            0,
            V16CuMarketParams {
                max_portfolio_assets: LEGS as u16,
                max_price_move_bps_per_slot: 10_000,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
            MAX_10M_MARKET_SLOTS,
        );
        let admin = env.admin.insecure_clone();
        let provider = Keypair::new();
        let insurer = Keypair::new();
        for owner in [&provider, &insurer] {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        }
        for asset in LEGS..MAX_10M_MARKET_SLOTS {
            let cu = env.activate_asset_with_authorities(
                asset as u16,
                asset as u64,
                100,
                insurer.pubkey(),
                admin.pubkey(),
                provider.pubkey(),
                admin.pubkey(),
            );
            assert_cu_within("dense market activation", cu, CUSTODY_CU_LIMIT);
        }
        for e in &exposures {
            if usize::from(e.asset) < LEGS {
                for (role, owner) in [
                    (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
                    (processor::ASSET_AUTH_INSURANCE, &insurer),
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(owner),
                        e.asset,
                        role,
                        owner.pubkey().to_bytes(),
                    )
                    .unwrap();
                }
            }
        }
        env.svm.warp_to_slot(START);
        for e in &exposures {
            env.configure_auth_mark_for_asset_as_admin(e.asset, START, 100);
        }
        let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
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
        let provider_token =
            create_ata_for_test(&mut env.svm, &env.payer, provider.pubkey(), env.mint);
        let insurer_token =
            create_ata_for_test(&mut env.svm, &env.payer, insurer.pubkey(), env.mint);
        let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let backing = exposures.iter().map(|e| e.backing).sum::<u64>();
        let insurance = exposures.iter().map(|e| e.insurance).sum::<u64>();
        for (token, amount) in tokens
            .into_iter()
            .map(|token| (token, CAPITAL))
            .chain([(provider_token, backing), (insurer_token, insurance)])
        {
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
        for i in 0..4 {
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL.into()),
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
        for e in &exposures {
            for (owner, token, instruction) in [
                (
                    &provider,
                    provider_token,
                    ProgInstruction::TopUpBackingBucket {
                        domain: e.domain() as u16,
                        market_id: env.asset_market_id(e.asset),
                        authority_epoch: env
                            .control_sequences(usize::from(e.asset))
                            .authority_epoch,
                        intent_id: 0,
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: e.backing.into(),
                        expiry_slot: EXPIRY,
                    },
                ),
                (
                    &insurer,
                    insurer_token,
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: e.domain() as u16,
                        market_id: env.asset_market_id(e.asset),
                        authority_epoch: env
                            .control_sequences(usize::from(e.asset))
                            .authority_epoch,
                        intent_id: 0,
                        amount: e.insurance.into(),
                    },
                ),
            ] {
                env.send(
                    instruction,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[owner],
                )
                .unwrap();
            }
        }
        for (i, e) in exposures.iter().enumerate() {
            let winner = 2 * (i / LEGS);
            env.trade_asset_with_cu(
                e.asset,
                &owners[winner],
                portfolios[winner],
                &owners[winner + 1],
                portfolios[winner + 1],
                i128::from(e.units) * POS_SCALE as i128 * if e.long { 1 } else { -1 },
                100,
                0,
            );
        }
        env.svm.warp_to_slot(START + 1);
        for e in &exposures {
            env.push_auth_mark_for_asset_as_admin(
                e.asset,
                START + 1,
                if e.long { 101 } else { 99 },
            );
        }
        let mut entitlement = [CAPITAL; 4];
        for pair in 0..2 {
            let local = &exposures[pair * LEGS..(pair + 1) * LEGS];
            let gain = local.iter().map(|e| e.units).sum::<u64>();
            entitlement[2 * pair] += gain;
            entitlement[2 * pair + 1] -= gain;
            for actor in [2 * pair, 2 * pair + 1] {
                env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: START + 1,
                        observations: crank_observations_for_assets(
                            &local.iter().map(|e| e.asset).collect::<Vec<_>>(),
                        ),
                    },
                );
                let account = env.portfolio_state(portfolios[actor]);
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(&account)),
                    LEGS as u32
                );
                assert_eq!(
                    account.capital.get() as i128 + account.pnl.get(),
                    i128::from(entitlement[actor])
                );
                let sources: Vec<_> = account
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied())
                    .collect();
                assert_eq!(sources.len(), if actor % 2 == 0 { LEGS } else { 0 });
                for source in sources {
                    let e = local
                        .iter()
                        .find(|e| e.domain() == source.domain.get() as usize)
                        .unwrap();
                    assert_eq!(
                        source.source_claim_bound_num.get(),
                        u128::from(e.units) * BOUND_SCALE
                    );
                    assert_eq!(source.source_claim_liened_num.get(), 0);
                }
            }
        }
        env.svm.warp_to_slot(END);
        let peak = env.resolve();
        env.svm.warp_to_slot(END + 3);
        let owners = owners.each_ref().map(Signer::pubkey);
        let mut tracked = vec![
            env.market,
            env.vault,
            env.mint,
            env.vault_authority,
            admin.pubkey(),
            provider.pubkey(),
            insurer.pubkey(),
            provider_token,
            insurer_token,
            destination,
        ];
        tracked.extend(owners);
        tracked.extend(portfolios);
        tracked.extend(tokens);
        Self {
            env,
            exposures,
            owners,
            portfolios,
            tokens,
            provider: provider.pubkey(),
            insurer: insurer.pubkey(),
            provider_token,
            insurer_token,
            destination,
            entitlement,
            supply: 4 * CAPITAL + backing + insurance,
            tracked,
            steps: 0,
            rollbacks: 0,
            rollback_payouts: 0,
            peak,
        }
    }

    fn wire(&self, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: ix.encode(),
        }
    }

    fn land(&mut self, ix: Instruction, admin: bool, rollback: bool) {
        self.env.svm.expire_blockhash();
        let mut instructions = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
            ix.clone(),
        ];
        if rollback {
            instructions.push(system_instruction::transfer(
                &self.env.payer.pubkey(),
                &self.provider,
                u64::MAX,
            ));
        }
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
        let meta = if rollback {
            let failure = result.expect_err("valid System suffix exceeds payer funds");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(3, InstructionError::Custom(1))
            );
            assert!(failure
                .meta
                .logs
                .iter()
                .any(|line| *line == format!("Program {} success", self.env.program_id)));
            assert_eq!(
                keys.iter()
                    .map(|key| self.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );
            self.rollbacks += 1;
            self.rollback_payouts += usize::from(
                !admin
                    && failure
                        .meta
                        .logs
                        .iter()
                        .any(|line| *line == format!("Program {} success", spl_token::ID)),
            );
            failure.meta
        } else {
            let meta = result.expect("input-derived dense terminal continuation");
            for (key, account) in keys.iter().zip(before) {
                if *key == self.env.payer.pubkey()
                    || !ix
                        .accounts
                        .iter()
                        .any(|m| m.pubkey == *key && m.is_writable)
                {
                    assert_eq!(
                        self.env.svm.get_account(key),
                        account,
                        "complete peer Account frame"
                    );
                }
            }
            self.steps += 1;
            meta
        };
        assert_cu_within(
            "dense terminal continuation",
            meta.compute_units_consumed,
            LIMIT,
        );
        self.peak = self.peak.max(meta.compute_units_consumed);
        self.custody();
    }

    fn custody(&self) {
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, self.supply);
        assert!(mint.mint_authority.is_none());
        let vault = self
            .env
            .svm
            .get_account(&self.env.vault)
            .filter(|a| a.lamports != 0)
            .map_or(0, |a| TokenAccount::unpack(&a.data).unwrap().amount);
        let wallets = self
            .tokens
            .into_iter()
            .chain([self.provider_token, self.insurer_token, self.destination])
            .map(|key| self.env.token_amount(key))
            .sum::<u64>();
        assert_eq!(vault + wallets, self.supply);
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
        for actor in 0..4 {
            assert!(self.env.token_amount(self.tokens[actor]) <= self.entitlement[actor]);
        }
        assert_eq!(self.env.token_amount(self.destination), 0);
    }

    // Each earlier obligation can move value into a later component of this rank.
    fn rank(&self, actor: usize) -> [u128; 7] {
        let a = self.env.portfolio_state(self.portfolios[actor]);
        [
            a.legs
                .iter()
                .filter(|leg| leg.try_to_runtime().unwrap().active)
                .count() as u128,
            a.source_domains
                .iter()
                .map(|s| s.source_claim_liened_num.get())
                .sum(),
            a.source_domains.iter().filter(|s| s.is_occupied()).count() as u128,
            a.pnl.get().min(0).unsigned_abs(),
            a.capital.get() + a.pnl.get().max(0) as u128,
            u128::from(
                self.entitlement[actor]
                    .checked_sub(self.env.token_amount(self.tokens[actor]))
                    .unwrap(),
            ),
            u128::from(END.checked_sub(a.last_fee_slot.get()).unwrap()),
        ]
    }

    fn payout(&self, actor: usize, crank: bool) -> Instruction {
        self.wire(
            if crank {
                ProgInstruction::PermissionlessCrank {
                    now_slot: END + 3,
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

    fn oi_census(&self) {
        let group = self.env.market_state().1;
        let mut oi = vec![[0u128; 2]; MAX_10M_MARKET_SLOTS];
        let mut capital = 0;
        for (actor, portfolio) in self.portfolios.iter().enumerate() {
            let a = self.env.portfolio_state(*portfolio);
            capital += a.capital.get();
            for leg in a
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
            {
                let e = self.exposures[actor / 2 * LEGS..(actor / 2 + 1) * LEGS]
                    .iter()
                    .find(|e| u32::from(e.asset) == leg.asset_index)
                    .unwrap();
                let long = e.long == (actor % 2 == 0);
                assert_eq!(
                    leg.basis_pos_q,
                    i128::from(e.units) * POS_SCALE as i128 * if long { 1 } else { -1 }
                );
                oi[usize::from(e.asset)][usize::from(!long)] += u128::from(e.units) * POS_SCALE;
            }
        }
        for (asset, expected) in group.assets.iter().zip(oi) {
            assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], expected);
        }
        assert_eq!(group.c_tot, capital);
    }

    fn advance(&mut self, actor: usize, crank: bool) {
        let before = self.rank(actor);
        self.land(self.payout(actor, crank), false, false);
        let after = self.rank(actor);
        assert!(after < before, "actor {actor}: {before:?} -> {after:?}");
        self.oi_census();
    }

    fn reserves(&self, backing_paid: &[u64], insurance_paid: &[u64]) {
        let market = self.env.svm.get_account(&self.env.market).unwrap();
        let (_, group) = state::read_market(&market.data).unwrap();
        let mut fresh = 0;
        let mut insurance = 0;
        for (domain, (bucket, source)) in group
            .source_backing_buckets
            .iter()
            .zip(&group.source_credit)
            .enumerate()
        {
            let i = self.exposures.iter().position(|e| e.domain() == domain);
            let backing = i.map_or(0, |i| self.exposures[i].backing - backing_paid[i]);
            let budget = i.map_or(0, |i| self.exposures[i].insurance - insurance_paid[i]);
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                u128::from(backing) * BOUND_SCALE,
                "domain {domain}"
            );
            assert_eq!(
                source.fresh_reserved_backing_num,
                u128::from(backing) * BOUND_SCALE
            );
            assert_eq!(source.positive_claim_bound_num, 0);
            assert_eq!(source.valid_liened_backing_num, 0);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            // Solvent loss credit replenishes fresh stock before the winner consumes
            // it. That conversion's audit counters survive principal withdrawal.
            let consumed = i.map_or(0, |i| u128::from(self.exposures[i].units) * BOUND_SCALE);
            assert_eq!(source.provider_receivable_num, consumed);
            assert_eq!(source.spent_backing_num, consumed);
            assert_eq!(bucket.consumed_liened_backing_num, consumed);
            assert_eq!(group.insurance_domain_budget[domain], u128::from(budget));
            assert_eq!(group.insurance_domain_spent[domain], 0);
            fresh += u128::from(backing) * BOUND_SCALE;
            insurance += u128::from(budget);
        }
        let header = market_group_header_bytes(&market.data);
        assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
        assert_eq!(
            header.insurance_domain_budget_remaining_total.get(),
            insurance
        );
        assert_eq!(group.insurance, insurance);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.vault, fresh / BOUND_SCALE + insurance);
        assert_eq!(
            self.env.token_amount(self.provider_token),
            backing_paid.iter().sum::<u64>()
        );
        assert_eq!(
            self.env.token_amount(self.insurer_token),
            insurance_paid.iter().sum::<u64>()
        );
    }
}

#[test]
fn v16_program_dense_claimants_and_reserves_reconcile_at_supported_capacity() {
    for (seed, crank) in [(0x4b00, false), (0x4b01, true)] {
        let mut w = DenseWorld::new(seed);
        assert_eq!(
            w.env.market_state().1.config.max_market_slots as usize,
            MAX_10M_MARKET_SLOTS
        );
        w.custody();
        w.oi_census();
        let order = if crank { [2, 3, 0, 1] } else { [0, 1, 2, 3] };
        // Detach every peer before selecting source realization or final payout.
        for _ in 0..LEGS {
            for actor in order {
                let before = w.rank(actor)[0];
                w.advance(actor, crank);
                assert_eq!(w.rank(actor)[0], before - 1);
            }
        }
        let settle = if crank { [3, 1, 2, 0] } else { [1, 3, 0, 2] };
        for actor in settle {
            for _ in 0..LEGS + 4 {
                if w.rank(actor) == [0; 7] {
                    break;
                }
                // The same public payout commits after a completely rolled-back transaction.
                let before = w.rank(actor);
                w.land(w.payout(actor, crank), false, true);
                assert_eq!(w.rank(actor), before);
                w.advance(actor, crank);
            }
            assert_eq!(w.rank(actor), [0; 7]);
            assert!(resolved_portfolio_is_terminal(&w.env, w.portfolios[actor]));
        }
        assert_eq!(w.tokens.map(|key| w.env.token_amount(key)), w.entitlement);
        for actor in order {
            let key = w.portfolios[actor];
            let rent = w.env.svm.get_account(&key).unwrap().lamports;
            let market_rent = w.env.svm.get_account(&w.env.market).unwrap().lamports;
            let count = w.env.market_state().1.materialized_portfolio_count;
            w.land(
                w.wire(
                    w.env.close_portfolio_ix(key),
                    vec![
                        AccountMeta::new(w.env.admin.pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                        AccountMeta::new(key, false),
                    ],
                ),
                true,
                false,
            );
            assert_eq!(
                w.env.market_state().1.materialized_portfolio_count,
                count - 1
            );
            assert_eq!(
                w.env.svm.get_account(&w.env.market).unwrap().lamports,
                market_rent + rent
            );
            assert!(w.env.svm.get_account(&key).is_none_or(|a| a.lamports == 0));
        }
        let mut backing_paid = vec![0; 2 * LEGS];
        let mut insurance_paid = vec![0; 2 * LEGS];
        w.reserves(&backing_paid, &insurance_paid);
        let mut reserve_order: Vec<_> = (0..2 * LEGS).collect();
        if crank {
            reserve_order.reverse();
        }
        let reserve_total = w.supply - 4 * CAPITAL;
        let unpaid_rank = |backing_paid: &[u64], insurance_paid: &[u64]| {
            reserve_total - backing_paid.iter().sum::<u64>() - insurance_paid.iter().sum::<u64>()
        };
        let mut reserve_rank = unpaid_rank(&backing_paid, &insurance_paid);
        for i in reserve_order {
            for provider in [!crank, crank] {
                let e = &w.exposures[i];
                let (owner, token, amount, ix) = if provider {
                    (
                        w.provider,
                        w.provider_token,
                        e.backing,
                        ProgInstruction::WithdrawBackingBucket {
                            domain: e.domain() as u16,
                            market_id: w.env.asset_market_id(e.asset),
                            authority_epoch: w
                                .env
                                .control_sequences(usize::from(e.asset))
                                .authority_epoch,
                            amount: e.backing.into(),
                        },
                    )
                } else {
                    (
                        w.insurer,
                        w.insurer_token,
                        e.insurance,
                        ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: e.asset,
                            market_id: w.env.asset_market_id(e.asset),
                            authority_epoch: w
                                .env
                                .control_sequences(usize::from(e.asset))
                                .authority_epoch,
                            amount: e.insurance.into(),
                        },
                    )
                };
                let before = w.env.token_amount(w.env.vault);
                w.land(
                    w.wire(
                        ix,
                        vec![
                            AccountMeta::new_readonly(owner, false),
                            AccountMeta::new(w.env.market, false),
                            AccountMeta::new(token, false),
                            AccountMeta::new(w.env.vault, false),
                            AccountMeta::new_readonly(w.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    ),
                    false,
                    false,
                );
                if provider {
                    backing_paid[i] += amount;
                } else {
                    insurance_paid[i] += amount;
                }
                assert_eq!(w.env.token_amount(w.env.vault), before - amount);
                let after = unpaid_rank(&backing_paid, &insurance_paid);
                assert!(after < reserve_rank, "input-derived unpaid reserve rank");
                assert_eq!(after, reserve_rank - amount);
                reserve_rank = after;
                w.reserves(&backing_paid, &insurance_paid);
            }
        }
        assert_eq!(reserve_rank, 0);
        assert_eq!(w.env.token_amount(w.env.vault), 0);
        // The independent stock census is empty: retirement needs no asset scan.
        assert_eq!(w.env.market_state().0.terminal_slab_scan_progress, 0);
        let ix = w.wire(
            ProgInstruction::CloseSlab {
                authority_epoch: w.env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(w.env.admin.pubkey(), true),
                AccountMeta::new(w.env.market, false),
                AccountMeta::new(w.env.vault, false),
                AccountMeta::new_readonly(w.env.vault_authority, false),
                AccountMeta::new(w.destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(w.env.mint, false),
            ],
        );
        let market = w.env.svm.get_account(&w.env.market).unwrap();
        let vault = w.env.svm.get_account(&w.env.vault).unwrap();
        let mut admin = w.env.svm.get_account(&w.env.admin.pubkey()).unwrap();
        w.land(ix.clone(), true, true);
        w.land(ix, true, false);
        let tombstone = w.env.svm.get_account(&w.env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.owner, w.env.program_id);
        let rent = w
            .env
            .svm
            .minimum_balance_for_rent_exemption(tombstone.data.len());
        assert_eq!(tombstone.lamports, rent);
        admin.lamports += market.lamports - rent + vault.lamports;
        assert_eq!(w.env.svm.get_account(&w.env.admin.pubkey()), Some(admin));
        assert!(w
            .env
            .svm
            .get_account(&w.env.vault)
            .is_none_or(|a| a.lamports == 0));
        w.custody();
        assert!(w.rollbacks >= 3);
        assert_eq!(w.rollback_payouts, 2);
        println!("Scope K seed={seed:#x}, crank={crank}, slots={MAX_10M_MARKET_SLOTS}, initial legs=56 sources=28, claimants={:?}, steps={}, rollbacks={}, rolled-back payouts={}, peak={} CU", w.entitlement, w.steps, w.rollbacks, w.rollback_payouts, w.peak);
    }
}
