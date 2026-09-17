//! Row 423: native custody composes with a full historical/latent source budget.
//! Uncredited SOL cannot buy source slots or disappear during keeper-driven exit.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeSet;

const ASSETS: usize = 14;
const DOMAINS: usize = 28;
const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const RAW: [u64; 3] = [17, 19, 23];
const LIMIT: u64 = 1_375_000;

struct NativeExit {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    empty: [Account; 3],
    mint: Account,
    passive: Vec<(Pubkey, Option<Account>)>,
    positions: [i128; ASSETS],
    claims: [u128; DOMAINS],
    previous: [u128; DOMAINS],
    slot: u64,
    calls: usize,
    rollbacks: usize,
    terminal_calls: usize,
    peak: u64,
    packet: usize,
}

impl NativeExit {
    fn new() -> Self {
        let mut env = inv081_public_native_market_with_params(
            ASSETS + 1,
            V16CuMarketParams {
                max_portfolio_assets: ASSETS as u16,
                initial_price: PRICE,
                max_price_move_bps_per_slot: 150,
                max_accrual_dt_slots: 64,
                min_funding_lifetime_slots: 64,
                ..V16CuMarketParams::default()
            },
        );
        env.portfolio_account_len =
            state::portfolio_account_len_for_market_slots(ASSETS + 1).unwrap();
        let mut peak = env.init_market_cu;
        for asset in 0..=ASSETS {
            if env.asset_market_id(asset as u16) == 0 {
                peak = peak.max(env.activate_asset(asset as u16, 0, PRICE));
            }
            peak = peak.max(env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, PRICE));
        }
        peak = peak.max(env.configure_permissionless_resolve_with_cu(1_000, 5));
        let owners = [Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 2];
        let mut tokens = [Pubkey::default(); 2];
        let vault_empty = env.svm.get_account(&env.vault).unwrap();
        let mut empty = std::array::from_fn(|_| vault_empty.clone());
        for actor in 0..2 {
            env.svm
                .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                .unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[actor] = portfolio.pubkey();
            peak = peak.max(
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap(),
            );
            env.portfolios.push(portfolios[actor]);
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            empty[actor + 1] = env.svm.get_account(&tokens[actor]).unwrap();
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::transfer(
                        &owners[actor].pubkey(),
                        &tokens[actor],
                        CAPITAL as u64,
                    ),
                    spl_token::instruction::sync_native(&spl_token::ID, &tokens[actor]).unwrap(),
                    system_instruction::transfer(
                        &owners[actor].pubkey(),
                        &tokens[actor],
                        RAW[actor + 1],
                    ),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            peak = peak.max(
                env.send(
                    env.deposit_ix(portfolios[actor], CAPITAL),
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
                .unwrap(),
            );
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            system_instruction::transfer(&env.admin.pubkey(), &env.vault, RAW[0]),
            &[&env.admin],
        )
        .unwrap();
        let passive = [
            env.admin.pubkey(),
            owners[0].pubkey(),
            owners[1].pubkey(),
            env.vault_authority,
        ]
        .map(|key| (key, env.svm.get_account(&key)))
        .to_vec();
        let mint = env.svm.get_account(&env.mint).unwrap();
        let s = Self {
            env,
            owners,
            portfolios,
            tokens,
            empty,
            mint,
            passive,
            positions: [0; ASSETS],
            claims: [0; DOMAINS],
            previous: [0; DOMAINS],
            slot: 0,
            calls: 0,
            rollbacks: 0,
            terminal_calls: 0,
            peak,
            packet: 0,
        };
        s.audit(false);
        s
    }

    fn instruction(&self, data: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: data.encode(),
        }
    }

    fn trade_ix(&self, asset: usize, quantity: i128, price: u64) -> Instruction {
        self.instruction(
            self.env.trade_no_cpi_ix(
                self.portfolios[0],
                self.portfolios[1],
                asset as u16,
                quantity,
                price,
                0,
            ),
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.owners[1].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[0], false),
                AccountMeta::new(self.portfolios[1], false),
            ],
        )
    }

    fn custody_ix(&self, actor: usize, data: ProgInstruction, signed: bool) -> Instruction {
        self.instruction(
            data,
            vec![
                AccountMeta::new_readonly(self.owners[actor].pubkey(), signed),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }

    fn signed(&self, instructions: &[Instruction], actors: &[usize]) -> Transaction {
        let mut signers = vec![&self.env.payer];
        signers.extend(actors.iter().map(|&actor| &self.owners[actor]));
        Transaction::new_signed_with_payer(
            &[
                &[
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
                ][..],
                instructions,
            ]
            .concat(),
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        )
    }

    #[track_caller]
    fn submit(
        &mut self,
        tx: Transaction,
        changed: &[Pubkey],
        error: Option<(PercolatorError, Pubkey)>,
    ) {
        tx.verify().unwrap();
        let bytes = bincode::serialized_size(&tx).unwrap() as usize;
        assert!(bytes <= 1232);
        self.packet = self.packet.max(bytes);
        let keys: BTreeSet<_> = tx
            .message
            .account_keys
            .iter()
            .copied()
            .chain(self.portfolios)
            .chain(self.tokens)
            .chain([
                self.env.market,
                self.env.vault,
                self.env.mint,
                self.env.admin.pubkey(),
                self.owners[0].pubkey(),
                self.owners[1].pubkey(),
                self.env.vault_authority,
            ])
            .collect();
        let before: Vec<_> = keys
            .iter()
            .map(|&key| (key, self.env.svm.get_account(&key)))
            .collect();
        let fee = tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
        let success = error.is_none();
        let result = self.env.svm.send_transaction(tx);
        let cu = if let Some((error, prefix_program)) = error {
            let failed = result.expect_err("capacity/terminal suffix must reject");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(3, InstructionError::Custom(error as u32)),
                "{failed:?}"
            );
            assert_eq!(
                failed
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {prefix_program} success"))
                    .count(),
                1,
                "the native sync or terminal payout prefix must succeed"
            );
            self.rollbacks += 1;
            failed.meta.compute_units_consumed
        } else {
            result
                .expect("reserved source resources retain public progress")
                .compute_units_consumed
        };
        for (key, mut expected) in before {
            if key == self.env.payer.pubkey() {
                expected.as_mut().unwrap().lamports -= fee;
            } else if success && changed.contains(&key) {
                continue;
            }
            assert_eq!(
                self.env.svm.get_account(&key),
                expected,
                "complete Account frame {key}"
            );
        }
        assert_cu_within("native latent capacity/exit", cu, LIMIT);
        self.peak = self.peak.max(cu);
        self.calls += 1;
    }

    fn source_claims(&self, actor: usize) -> [u128; DOMAINS] {
        let mut claims = [0; DOMAINS];
        let mut seen = BTreeSet::new();
        for source in self
            .env
            .portfolio_state(self.portfolios[actor])
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
        {
            let domain = source.domain.get() as usize;
            assert!(domain < DOMAINS && seen.insert(domain));
            assert_eq!(
                source.source_claim_market_id.get(),
                self.env.asset_market_id((domain / 2) as u16)
            );
            assert_eq!(source.source_claim_liened_num.get(), 0);
            assert_eq!(source.source_lien_counterparty_backing_num.get(), 0);
            assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
            claims[domain] = source.source_claim_bound_num.get();
        }
        claims
    }

    fn audit(&self, terminal: bool) {
        let market = self.env.svm.get_account(&self.env.market).unwrap();
        let group = self.env.market_state().1;
        let accounts = self.portfolios.map(|p| self.env.portfolio_state(p));
        assert_market_stock_census(
            "native latent exit",
            &group,
            &market.data,
            &accounts,
            self.env.token_amount(self.env.vault) as u128,
        )
        .unwrap();
        assert_reservation_encumbrance_census("native latent exit", &group, &accounts).unwrap();
        assert_source_credit_rates("native latent exit", &group).unwrap();
        let claims = self.source_claims(0);
        assert_eq!(self.source_claims(1), [0; DOMAINS]);
        for domain in 0..DOMAINS {
            let source = &group.source_credit[domain];
            let bucket = &group.source_backing_buckets[domain];
            let old = if terminal {
                0
            } else {
                self.previous[domain] * BOUND_SCALE
            };
            let next = self.claims[domain] * BOUND_SCALE;
            assert!(claims[domain] == old || claims[domain] == next);
            assert_eq!(source.exact_positive_claim_num, claims[domain]);
            assert_eq!(source.positive_claim_bound_num, claims[domain]);
            assert!(
                bucket.fresh_unliened_backing_num == old
                    || bucket.fresh_unliened_backing_num == next
            );
            assert_eq!(
                source.fresh_reserved_backing_num,
                bucket.fresh_unliened_backing_num
            );
            assert_eq!(
                (
                    source.valid_liened_backing_num,
                    source.impaired_liened_backing_num,
                    source.insurance_credit_reserved_num
                ),
                (0, 0, 0)
            );
            assert!(
                claims[domain] * source.credit_rate_num / percolator::CREDIT_RATE_SCALE
                    <= bucket.fresh_unliened_backing_num
            );
            if terminal {
                assert_eq!(bucket.fresh_unliened_backing_num, claims[domain]);
            }
        }
        for domain in DOMAINS..DOMAINS + 2 {
            assert_eq!(
                group.source_credit[domain],
                percolator::SourceCreditStateV16::EMPTY
            );
            assert_eq!(
                group.source_backing_buckets[domain].fresh_unliened_backing_num,
                0
            );
            assert_eq!(group.insurance_domain_budget[domain], 0);
        }
        let paid = self.tokens.map(|key| self.env.token_amount(key) as u128);
        let gain = self.claims.iter().sum::<u128>();
        for (actor, entitlement) in [CAPITAL + gain, CAPITAL - gain].into_iter().enumerate() {
            assert!(paid[actor] == 0 || (terminal && paid[actor] == entitlement));
        }
        assert_eq!(group.vault, 2 * CAPITAL - paid.iter().sum::<u128>());
        assert_eq!(group.insurance, 0);
        for (index, key) in [self.env.vault, self.tokens[0], self.tokens[1]]
            .into_iter()
            .enumerate()
        {
            let amount = if index == 0 {
                group.vault
            } else {
                paid[index - 1]
            } as u64;
            let mut expected = self.empty[index].clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            assert_eq!(token.amount, 0);
            assert_eq!(token.is_native, COption::Some(expected.lamports));
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            expected.lamports += amount + RAW[index];
            assert_eq!(
                self.env.svm.get_account(&key),
                Some(expected),
                "native amount, lamports, rent and raw donation {key}"
            );
        }
        assert_eq!(
            self.env.svm.get_account(&self.env.mint),
            Some(self.mint.clone())
        );
        for (key, account) in &self.passive {
            assert_eq!(&self.env.svm.get_account(key), account);
        }
        if !terminal {
            assert_eq!(accounts[0].capital.get(), CAPITAL);
            assert_eq!(
                accounts[0].pnl.get(),
                (claims.iter().sum::<u128>() / BOUND_SCALE) as i128
            );
            let backing = group.source_backing_buckets[..DOMAINS]
                .iter()
                .map(|b| b.fresh_unliened_backing_num / BOUND_SCALE)
                .sum::<u128>();
            assert_eq!(accounts[1].capital.get(), CAPITAL - backing);
            assert_eq!(accounts[1].pnl.get(), 0);
            assert_eq!(group.c_tot, 2 * CAPITAL - backing);
            for (actor, account) in accounts.iter().enumerate() {
                let mut resources: BTreeSet<_> = account
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied())
                    .map(|s| s.domain.get() as usize)
                    .collect();
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(account)) as usize,
                    self.positions.iter().filter(|&&q| q != 0).count()
                );
                for (asset, &q) in self.positions.iter().enumerate() {
                    assert_eq!(has_active_leg_for_asset(account, asset), q != 0);
                    if q != 0 {
                        assert_eq!(
                            active_leg_for_asset(account, asset).basis_pos_q,
                            q * if actor == 0 { 1 } else { -1 }
                        );
                        resources.extend([2 * asset, 2 * asset + 1]);
                    }
                    assert_eq!(group.assets[asset].oi_eff_long_q, q.unsigned_abs());
                    assert_eq!(group.assets[asset].oi_eff_short_q, q.unsigned_abs());
                }
                assert!(resources.len() <= DOMAINS);
            }
        }
    }

    fn trade(&mut self, asset: usize, quantity: i128, price: u64) {
        self.env.svm.expire_blockhash();
        let tx = self.signed(&[self.trade_ix(asset, quantity, price)], &[0, 1]);
        self.submit(
            tx,
            &[self.env.market, self.portfolios[0], self.portfolios[1]],
            None,
        );
        self.positions[asset] += quantity;
        self.audit(false);
    }

    fn settle(&mut self, asset: usize, before_price: u64, price: u64, order: [usize; 2]) {
        let q = self.positions[asset] / POS_SCALE as i128;
        let gain = q * (price as i128 - before_price as i128);
        assert!(gain > 0);
        self.previous = self.claims;
        self.claims[2 * asset + usize::from(q > 0)] += gain as u128;
        self.slot += 1;
        self.env.svm.warp_to_slot(self.slot);
        self.peak = self.peak.max(self.env.push_auth_mark_for_asset_as_admin(
            asset as u16,
            self.slot,
            price,
        ));
        self.audit(false);
        let total = self.claims.iter().sum::<u128>();
        for actor in order {
            let rank = |s: &Self| {
                let group = s.env.market_state().1;
                let a = s.env.portfolio_state(s.portfolios[actor]);
                u128::from(s.slot - group.assets[asset].slot_last)
                    + if actor == 0 {
                        a.pnl.get().abs_diff(total as i128)
                    } else {
                        a.capital.get().abs_diff(CAPITAL - total)
                    }
            };
            for _ in 0..4 {
                let before = rank(self);
                if before == 0 {
                    break;
                }
                self.env.svm.expire_blockhash();
                let ix = self.instruction(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: self.slot,
                        observations: crank_observations(asset as u16),
                    },
                    vec![
                        AccountMeta::new_readonly(self.env.payer.pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(self.portfolios[actor], false),
                    ],
                );
                self.submit(
                    self.signed(&[ix], &[]),
                    &[self.env.market, self.portfolios[actor]],
                    None,
                );
                self.audit(false);
                assert!(
                    rank(self) < before,
                    "settlement strictly reduces input-derived debt"
                );
            }
            assert_eq!(rank(self), 0);
        }
        self.previous = self.claims;
        assert_eq!(self.source_claims(0), self.claims.map(|c| c * BOUND_SCALE));
        self.audit(false);
    }

    fn frontier(&self, occupied: usize, latent: usize) {
        self.audit(false);
        let claims = self.source_claims(0);
        assert_eq!(claims.iter().filter(|&&claim| claim > 0).count(), occupied);
        let mut union: BTreeSet<_> = self
            .claims
            .iter()
            .enumerate()
            .filter_map(|(d, &c)| (c > 0).then_some(d))
            .collect();
        for (asset, &q) in self.positions.iter().enumerate() {
            if q != 0 {
                union.extend([2 * asset, 2 * asset + 1]);
            }
        }
        assert_eq!(union.len() - occupied, latent);
        assert_eq!(
            occupied + latent,
            DOMAINS,
            "input-derived source reservation is full"
        );
        union.extend([DOMAINS, DOMAINS + 1]);
        assert_eq!(
            union.len(),
            DOMAINS + 2,
            "spare admission exceeds the source budget even with native surplus"
        );
    }
}

#[test]
fn v16_program_native_full_source_reservation_preserves_rollback_and_redeemable_exit() {
    assert_eq!(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize,
        ASSETS
    );
    assert_eq!(
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS,
        DOMAINS
    );
    assert!(
        include_str!("../../../Cargo.lock").contains("4db11a8cb0053815e23a35d3a7d3edc265d8d866")
    );
    let mut maxima = [0; 2];
    let mut totals = [0; 3];
    for direction in [-1i128, 1] {
        for order in [[0, 1], [1, 0]] {
            let mut s = NativeExit::new();
            for asset in 0..ASSETS - 1 {
                let q = direction * (1 + (asset % 3) as i128) * POS_SCALE as i128;
                let moved = (PRICE as i128 + direction) as u64;
                s.trade(asset, q, PRICE);
                s.settle(asset, PRICE, moved, order);
                s.trade(asset, -2 * q, moved);
                s.settle(asset, moved, PRICE, [order[1], order[0]]);
                s.trade(asset, q, PRICE);
            }
            assert_eq!(s.claims.iter().sum::<u128>(), 50);
            let live = ASSETS - 1;
            let q = direction * 4 * POS_SCALE as i128;
            s.trade(live, q, PRICE);
            s.frontier(26, 2);
            let spare_asset = s.env.market_state().1.assets[ASSETS];
            s.env.svm.expire_blockhash();
            let denied = s.signed(
                &[
                    spl_token::instruction::sync_native(&spl_token::ID, &s.env.vault).unwrap(),
                    s.trade_ix(ASSETS, POS_SCALE as i128, PRICE),
                ],
                &[0, 1],
            );
            s.submit(
                denied,
                &[],
                Some((PercolatorError::InvalidInstruction, spl_token::ID)),
            );
            s.frontier(26, 2);
            let moved = (PRICE as i128 + direction) as u64;
            s.settle(live, PRICE, moved, order);
            s.frontier(27, 1);
            s.trade(live, -2 * q, moved);
            s.settle(live, moved, PRICE, [order[1], order[0]]);
            s.frontier(28, 0);
            s.trade(live, q, PRICE);
            assert_eq!(s.claims.iter().sum::<u128>(), 58);
            assert_eq!(s.env.market_state().1.assets[ASSETS], spare_asset);
            let portfolios = s.portfolios.map(|key| s.env.svm.get_account(&key));
            s.peak = s.peak.max(s.env.resolve());
            assert_eq!(
                s.portfolios.map(|key| s.env.svm.get_account(&key)),
                portfolios
            );
            assert_eq!(s.env.market_state().1.mode, MarketModeV16::Resolved);
            s.slot += 5;
            s.env.svm.warp_to_slot(s.slot);
            let entitlement = [CAPITAL + 58, CAPITAL - 58];
            for actor in order {
                let rank = |s: &NativeExit| {
                    s.source_claims(actor).iter().filter(|&&c| c > 0).count()
                        + usize::from(
                            (s.env.token_amount(s.tokens[actor]) as u128) < entitlement[actor],
                        )
                };
                for step in 0..=DOMAINS {
                    let before = rank(&s);
                    if before == 0 {
                        break;
                    }
                    s.env.svm.expire_blockhash();
                    let close = s.custody_ix(
                        actor,
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        false,
                    );
                    let retry = s.signed(&[close.clone()], &[]);
                    assert_eq!(retry.signatures.len(), 1);
                    let wire = bincode::serialize(&retry).unwrap();
                    // At most one source remains here, so this prefix really pays native tokens.
                    if before <= 2 {
                        let withdrawal =
                            s.custody_ix(actor, s.env.withdraw_ix(s.portfolios[actor], 1), true);
                        let denied = s.signed(&[close, withdrawal], &[actor]);
                        s.submit(
                            denied,
                            &[],
                            Some((PercolatorError::EngineLockActive, s.env.program_id)),
                        );
                        s.audit(true);
                    }
                    let paid_before = s.env.token_amount(s.tokens[actor]);
                    let vault_before = s.env.token_amount(s.env.vault);
                    assert_eq!(bincode::serialize(&retry).unwrap(), wire);
                    s.submit(
                        retry,
                        &[
                            s.env.market,
                            s.portfolios[actor],
                            s.tokens[actor],
                            s.env.vault,
                        ],
                        None,
                    );
                    let paid = s.env.token_amount(s.tokens[actor]) - paid_before;
                    assert_eq!(vault_before - s.env.token_amount(s.env.vault), paid);
                    if before <= 2 {
                        assert!(
                            paid > 0,
                            "rolled-back prefix must include a real native payout"
                        );
                    }
                    assert!(
                        rank(&s) < before,
                        "terminal call {step} strictly decreases source/payment debt"
                    );
                    s.terminal_calls += 1;
                    s.audit(true);
                }
                assert_eq!(rank(&s), 0, "bounded complete payout");
                assert!(resolved_portfolio_is_terminal(&s.env, s.portfolios[actor]));
            }
            let market_rent_before = s.env.svm.get_account(&s.env.market).unwrap().lamports;
            let portfolio_rent: u64 = s
                .portfolios
                .iter()
                .map(|p| s.env.svm.get_account(p).unwrap().lamports)
                .sum();
            for actor in order {
                s.env.svm.expire_blockhash();
                let close = s.instruction(
                    s.env.close_portfolio_ix(s.portfolios[actor]),
                    vec![
                        AccountMeta::new(s.owners[actor].pubkey(), true),
                        AccountMeta::new(s.env.market, false),
                        AccountMeta::new(s.portfolios[actor], false),
                    ],
                );
                s.submit(
                    s.signed(&[close], &[actor]),
                    &[s.env.market, s.portfolios[actor]],
                    None,
                );
                assert!(s
                    .env
                    .svm
                    .get_account(&s.portfolios[actor])
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            }
            let group = s.env.market_state().1;
            assert_eq!(
                (
                    group.vault,
                    group.c_tot,
                    group.insurance,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0, 0)
            );
            assert_eq!(
                s.env.svm.get_account(&s.env.market).unwrap().lamports,
                market_rent_before + portfolio_rent
            );
            assert!(group
                .assets
                .iter()
                .all(|a| a.oi_eff_long_q == 0 && a.oi_eff_short_q == 0));
            let market_frame = s.env.svm.get_account(&s.env.market);
            let vault_frame = s.env.svm.get_account(&s.env.vault);
            assert_eq!(
                vault_frame.as_ref().unwrap().lamports,
                s.empty[0].lamports + RAW[0]
            );
            for actor in order {
                let wallet_before = s
                    .env
                    .svm
                    .get_account(&s.owners[actor].pubkey())
                    .unwrap()
                    .lamports;
                let amount = entitlement[actor] as u64;
                assert_eq!(s.env.token_amount(s.tokens[actor]), amount);
                s.env.svm.expire_blockhash();
                let redeem = spl_token::instruction::close_account(
                    &spl_token::ID,
                    &s.tokens[actor],
                    &s.owners[actor].pubkey(),
                    &s.owners[actor].pubkey(),
                    &[],
                )
                .unwrap();
                s.submit(
                    s.signed(&[redeem], &[actor]),
                    &[s.tokens[actor], s.owners[actor].pubkey()],
                    None,
                );
                assert_eq!(
                    s.env
                        .svm
                        .get_account(&s.owners[actor].pubkey())
                        .unwrap()
                        .lamports
                        - wallet_before,
                    amount + RAW[actor + 1] + s.empty[actor + 1].lamports
                );
                assert!(s
                    .env
                    .svm
                    .get_account(&s.tokens[actor])
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                assert_eq!(s.env.svm.get_account(&s.env.market), market_frame);
                assert_eq!(s.env.svm.get_account(&s.env.vault), vault_frame);
                assert_eq!(s.env.svm.get_account(&s.env.mint), Some(s.mint.clone()));
            }
            maxima[0] = maxima[0].max(s.peak);
            maxima[1] = maxima[1].max(s.packet as u64);
            totals[0] += s.calls;
            totals[1] += s.rollbacks;
            totals[2] += s.terminal_calls;
        }
    }
    assert_eq!(totals[1], 12);
    println!("INV-028 native latent exit: worlds=4, checked_transactions={}, exact_rollbacks={}, terminal_calls={}, max_cu={}, max_packet={}", totals[0], totals[1], totals[2], maxima[0], maxima[1]);
}
