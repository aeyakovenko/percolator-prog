//! INV-045/024/036/041/062/088, row 422: a clipped, self-rewarded maintenance
//! collection composes with two Hybrid liquidation receipts. Cross collection
//! before/after the first receipt, shared target/keeper ownership, and two fee
//! shares. The input-owned keeper book distinguishes forgiven fees, maintenance
//! rebates, source penalties and exact SPL payout; ownership must not pool value.
//! System/SPL/wrapper construction only. No CPI route switching or same-slot
//! report replacement. Finite conformance; row 422 and INV-045 stay open.

use super::*;

const SEED: u128 = 101;
const FINAL: u64 = 980_000;
const ACTION_CU: u64 = 500_000;

#[derive(Debug, PartialEq, Eq)]
struct KeeperBook {
    capital: u128,
    slot: u64,
    charged: u128,
    rebates: u128,
    forgiven: u128,
    maintenance_budgets: [u128; 2],
    penalties: u128,
    rewards: u128,
    liquidation_budgets: [u128; 2],
}

impl KeeperBook {
    fn new() -> Self {
        Self {
            capital: SEED,
            slot: 1,
            charged: 0,
            rebates: 0,
            forgiven: 0,
            maintenance_budgets: [0; 2],
            penalties: 0,
            rewards: 0,
            liquidation_budgets: [0; 2],
        }
    }

    fn collect(&mut self, slot: u64, share: u128) -> (u128, u128) {
        let due = RATE * u128::from(slot - self.slot);
        let charged = due.min(self.capital);
        let rebate = charged * share / 10_000;
        let retained = charged - rebate;
        self.capital -= retained;
        self.slot = slot;
        self.charged += charged;
        self.rebates += rebate;
        self.forgiven += due - charged;
        self.maintenance_budgets[0] += retained / 2;
        self.maintenance_budgets[1] += retained.div_ceil(2);
        (charged, rebate)
    }

    fn reward(&mut self, closed: u128, price: u64) -> (u128, u128) {
        let penalty = fee(closed, price, 5);
        let reward = penalty * SHARE / 10_000;
        self.penalties += penalty;
        self.rewards += reward;
        self.capital += reward;
        self.liquidation_budgets[0] += (penalty - reward) / 2;
        self.liquidation_budgets[1] += (penalty - reward).div_ceil(2);
        (penalty, reward)
    }

    fn check(&self, env: &V16CuEnv, portfolios: [Pubkey; 5], discovery: u128) {
        let accounts = portfolios.map(|key| env.portfolio_state(key));
        let keeper = accounts[4];
        assert_eq!(keeper.capital.get(), self.capital);
        assert_eq!(keeper.last_fee_slot.get(), self.slot);
        assert_eq!(keeper.pnl.get(), 0);
        assert!(accounts
            .iter()
            .all(|account| account.fee_credits.get() == 0));
        // The four solvent positions pay full, even charges at their fee cursors.
        // Phase boundaries below separately require every cursor to reach Clock.
        let peer_fees = accounts[..4]
            .iter()
            .map(|account| RATE * u128::from(account.last_fee_slot.get() - 1))
            .sum::<u128>();
        let group = env.market_state().1;
        let retained = self.charged - self.rebates;
        assert_eq!(
            group.insurance,
            discovery + peer_fees + retained + self.penalties - self.rewards
        );
        assert_eq!(
            &group.insurance_domain_budget[..4],
            &[
                peer_fees / 2 + self.maintenance_budgets[0],
                peer_fees / 2 + self.maintenance_budgets[1],
                self.liquidation_budgets[0],
                self.liquidation_budgets[1],
            ]
        );
        assert!(group.insurance_domain_budget[4..].iter().all(|x| *x == 0));
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            peer_fees + retained + self.penalties - self.rewards
        );
        assert_eq!(
            group.insurance - group.insurance_domain_budget_remaining_total,
            discovery,
            "neither maintenance rebates nor liquidation can recycle paid discovery"
        );
        census(env, portfolios);
    }
}

fn invalid_suffix() -> Instruction {
    Instruction {
        program_id: solana_sdk::system_program::ID,
        accounts: vec![],
        data: vec![255],
    }
}

fn checked(
    env: &mut V16CuEnv,
    owner: &Keypair,
    instructions: &[Instruction],
    tracked: &[Pubkey],
    rejection: Option<(u8, InstructionError)>,
) -> u64 {
    submit_with_cu_limit(env, owner, instructions, tracked, rejection, ACTION_CU)
}

fn maintenance_ix(env: &V16CuEnv, keeper: Pubkey) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper, false),
            AccountMeta::new(keeper, false),
        ],
        data: ProgInstruction::SyncMaintenanceFee { now_slot: u64::MAX }.encode(),
    }
}

fn collect(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolios: [Pubkey; 5],
    tracked: &[Pubkey],
    book: &mut KeeperBook,
    share: u128,
) -> u64 {
    let keeper = portfolios[4];
    let ix = maintenance_ix(env, keeper);
    let mut peak = checked(
        env,
        owner,
        &[ix.clone(), invalid_suffix()],
        tracked,
        Some((3, InstructionError::InvalidInstructionData)),
    );
    let group = env.market_state().1;
    let peers = portfolios[..4]
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap();
    let (charged, rebate) = book.collect(env.svm.get_sysvar::<Clock>().slot, share);
    assert!(charged > 0 && rebate > 0);
    peak = peak.max(checked(env, owner, &[ix], tracked, None));
    assert_eq!(env.portfolio_state(keeper).capital.get(), book.capital);
    assert_eq!(env.portfolio_state(keeper).last_fee_slot.get(), book.slot);
    assert_eq!(env.portfolio_state(keeper).fee_credits.get(), 0);
    let after = env.market_state().1;
    assert_eq!(after.insurance, group.insurance + charged - rebate);
    assert_eq!(after.c_tot + charged - rebate, group.c_tot);
    assert_eq!(after.assets, group.assets);
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap(),
        profile
    );
    assert_eq!(
        portfolios[..4]
            .iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>(),
        peers,
        "sharing an owner cannot debit the target or credit its portfolio"
    );
    assert_eq!(
        [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
        custody
    );
    peak
}

#[test]
fn v16_program_clipped_self_maintenance_keeps_hybrid_rewards_and_shared_owner_sources_exact() {
    let first = ENTRY - ENTRY * 24 / 10_000;
    let second = first - first * 24 / 10_000;
    let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
    let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
    let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
    let mut peak = 0;
    let mut liquidations = 0;
    let mut rollbacks = 0;
    for maintenance_share in [3_333u128, 5_000] {
        let mut order_outcomes = Vec::new();
        for collect_first in [false, true] {
            let mut reference = None;
            for common_owner in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        maintenance_fee_per_slot: RATE,
                        max_abs_funding_e9_per_slot: 0,
                        ..production_risk_params()
                    },
                );
                set_test_clock(&mut env, 1, 100);
                env.configure_auth_mark_for_asset_as_admin(0, 1, ENTRY);
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                env.update_maintenance_fee_policy_with_cu(maintenance_share as u16);
                let feed = [0x93; 32];
                let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
                env.try_configure_hybrid_asset_with_conf_filter_cu(
                    1,
                    1,
                    0,
                    [feed, [0; 32], [0; 32]],
                    &[initial],
                    1,
                    100,
                    0,
                    0,
                    1,
                    0,
                )
                .unwrap();
                assert_eq!(env.market_state().0.fee_redirect_to_market_0_bps, 0);
                let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
                let actor_owners: [&Keypair; 5] =
                    std::array::from_fn(|i| &owners[if common_owner && i == 4 { 0 } else { i }]);
                let mut funds = FUNDS;
                funds[4] = SEED as u64;
                let funded =
                    std::array::from_fn::<_, 5, _>(|i| fund(&mut env, actor_owners[i], funds[i]));
                let portfolios = funded.map(|pair| pair.0);
                let tokens = funded.map(|pair| pair.1);
                let [target, peer, trader_a, trader_b, keeper] = portfolios;
                assert_eq!(tokens[0] == tokens[4], common_owner);
                assert_ne!(target, keeper);
                env.trade_asset_with_cu(
                    1,
                    actor_owners[0],
                    target,
                    actor_owners[1],
                    peer,
                    (100 * POS_SCALE) as i128,
                    ENTRY,
                    0,
                );
                let mut tracked =
                    vec![env.market, env.mint, env.vault, initial, env.admin.pubkey()];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                let mint = env.svm.get_account(&env.mint);
                let supply = funds.iter().map(|amount| u128::from(*amount)).sum::<u128>();
                let mut book = KeeperBook::new();
                set_test_clock(&mut env, 5, 1_000);
                let advance = observe(&env, trader_a, actor_owners[4].pubkey(), initial, None);
                peak = peak.max(checked(
                    &mut env,
                    actor_owners[4],
                    &[advance],
                    &tracked,
                    None,
                ));
                env.trade_asset_with_cu(
                    1,
                    actor_owners[2],
                    trader_a,
                    actor_owners[3],
                    trader_b,
                    POS_SCALE as i128,
                    900_000,
                    0,
                );
                assert_eq!(env.market_state().1.assets[1].effective_price, ENTRY);
                assert_eq!(env.market_state().1.assets[1].raw_oracle_target_price, MARK);
                book.check(&env, portfolios, discovery);
                let mut episodes = Vec::new();
                for (phase, slot, raw, price) in [
                    (0, 6, MARK, first),
                    (1, 7, FINAL, second),
                    (2, 14, FINAL, FINAL),
                ] {
                    let now = 995 + slot as i64;
                    set_test_clock(&mut env, slot, now);
                    let report = env.set_pyth_price_with_conf(&feed, raw as i64, -6, 0, now);
                    tracked.push(report);
                    if phase == 0 && collect_first {
                        peak = peak.max(collect(
                            &mut env,
                            actor_owners[4],
                            portfolios,
                            &tracked,
                            &mut book,
                            maintenance_share,
                        ));
                        rollbacks += 1;
                        assert_eq!(book.forgiven, 5 * RATE - SEED);
                        assert_eq!(book.charged, SEED);
                        book.check(&env, portfolios, discovery);
                    }
                    let crank =
                        observe(&env, target, actor_owners[4].pubkey(), report, Some(keeper));
                    let mut rewarded = false;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        let before_target = env.portfolio_state(target);
                        if phase == 2
                            && before.assets[1].effective_price == price
                            && census(&env, portfolios)[0]
                            && health_cert(&before_target).certified_liq_deficit == 0
                        {
                            break;
                        }
                        let recipient = env.portfolio_state(keeper);
                        let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                        let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
                        if health_cert(&before_target).valid
                            && health_cert(&before_target).certified_liq_deficit > 0
                        {
                            peak = peak.max(checked(
                                &mut env,
                                actor_owners[4],
                                &[crank.clone(), invalid_suffix()],
                                &tracked,
                                Some((3, InstructionError::InvalidInstructionData)),
                            ));
                            rollbacks += 1;
                        }
                        peak = peak.max(checked(
                            &mut env,
                            actor_owners[4],
                            &[crank.clone()],
                            &tracked,
                            None,
                        ));
                        let after = env.market_state().1;
                        assert_eq!(after.assets[1].effective_price, price);
                        assert_eq!(after.assets[1].raw_oracle_target_price, raw);
                        let profile = state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            1,
                        )
                        .unwrap();
                        assert_eq!(profile.oracle_target_publish_time, now);
                        assert_eq!(profile.last_good_oracle_slot, slot);
                        assert_eq!(after.assets[0], before.assets[0]);
                        assert_eq!(
                            [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                            peers
                        );
                        assert_eq!(
                            [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                            custody
                        );
                        let closed = before.assets[1].oi_eff_long_q - after.assets[1].oi_eff_long_q;
                        let (penalty, reward) = book.reward(closed, price);
                        let mut expected_recipient = recipient;
                        expected_recipient.capital =
                            percolator::V16PodU128::new(recipient.capital.get() + reward);
                        if reward > 0 {
                            expected_recipient.health_cert.valid = 0;
                        }
                        assert_eq!(env.portfolio_state(keeper), expected_recipient,
                            "Hybrid receipt cannot collect, refund or restore recipient maintenance");
                        if closed > 0 {
                            assert!(phase < 2 && reward > 0 && closed < 100 * POS_SCALE);
                            assert_eq!(before_target.last_fee_slot.get(), slot);
                            assert!(health_cert(&before_target).certified_liq_deficit > 0);
                            for wrong in [ENTRY, MARK, raw, ACCEPTED_PRINT, 900_000] {
                                assert_ne!(penalty, fee(closed, wrong, 5));
                            }
                            assert_eq!(
                                values(&env, portfolios)[0],
                                before_target.capital.get() as i128 + before_target.pnl.get()
                                    - penalty as i128
                            );
                            assert_eq!(after.insurance, before.insurance + penalty - reward);
                            episodes.push((closed, penalty, reward));
                            liquidations += 1;
                            rewarded = true;
                        }
                        book.check(&env, portfolios, discovery);
                        if rewarded {
                            break;
                        }
                    }
                    assert_eq!(rewarded, phase < 2, "catchup cannot add a third receipt");
                    if phase != 0 || !collect_first {
                        peak = peak.max(collect(
                            &mut env,
                            actor_owners[4],
                            portfolios,
                            &tracked,
                            &mut book,
                            maintenance_share,
                        ));
                        rollbacks += 1;
                    }
                    for _ in 0..8 {
                        for i in [1, 2, 3, 0] {
                            if !census(&env, portfolios)[i] {
                                let refresh = observe(
                                    &env,
                                    portfolios[i],
                                    actor_owners[4].pubkey(),
                                    report,
                                    None,
                                );
                                peak = peak.max(checked(
                                    &mut env,
                                    actor_owners[4],
                                    &[refresh],
                                    &tracked,
                                    None,
                                ));
                                book.check(&env, portfolios, discovery);
                            }
                        }
                        if census(&env, portfolios)[..4].iter().all(|current| *current) {
                            break;
                        }
                    }
                    assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
                    assert!(portfolios.iter().all(|key| env
                        .portfolio_state(*key)
                        .last_fee_slot
                        .get()
                        == slot));
                    book.check(&env, portfolios, discovery);
                    let stable = tracked
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>();
                    let retry = maintenance_ix(&env, keeper);
                    peak = peak.max(checked(&mut env, actor_owners[4], &[retry], &tracked, None));
                    assert_eq!(tracked.iter().map(|key| env.svm.get_account(key)).collect::<Vec<_>>(), stable,
                        "post-reward same-slot retry cannot collect the rebate or resurrect forgiven fees");
                    peak = peak.max(checked(
                        &mut env,
                        actor_owners[4],
                        &[crank],
                        &tracked,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                    ));
                    rollbacks += 1;
                }
                assert_eq!(
                    book.forgiven,
                    if collect_first { 5 * RATE - SEED } else { 0 }
                );
                assert_eq!(book.charged + book.forgiven, 13 * RATE);
                assert_eq!(
                    book.capital,
                    SEED + book.rewards + book.rebates - book.charged
                );
                let payout = book.capital;
                assert!(payout > SEED && book.rewards > book.rebates);
                let before_payout = env.market_state().1;
                let peers = portfolios[..4]
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>();
                let withdraw = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(actor_owners[4].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(keeper, false),
                        AccountMeta::new(tokens[4], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(keeper, payout).encode(),
                };
                peak = peak.max(checked(
                    &mut env,
                    actor_owners[4],
                    &[withdraw.clone(), invalid_suffix()],
                    &tracked,
                    Some((3, InstructionError::InvalidInstructionData)),
                ));
                rollbacks += 1;
                peak = peak.max(checked(
                    &mut env,
                    actor_owners[4],
                    &[withdraw],
                    &tracked,
                    None,
                ));
                book.capital = 0;
                book.check(&env, portfolios, discovery);
                assert_eq!(
                    portfolios[..4]
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>(),
                    peers
                );
                for token in tokens {
                    assert_eq!(
                        u128::from(env.token_amount(token)),
                        if token == tokens[4] { payout } else { 0 }
                    );
                }
                let mut destinations = tokens.to_vec();
                destinations.sort_unstable();
                destinations.dedup();
                let paid = destinations
                    .iter()
                    .map(|key| u128::from(env.token_amount(*key)))
                    .sum::<u128>();
                assert_eq!(
                    paid, payout,
                    "shared ATA is one custody account, not two payments"
                );
                assert_eq!(env.svm.get_account(&env.mint), mint);
                let group = env.market_state().1;
                assert_eq!(group.assets, before_payout.assets);
                assert_eq!(group.insurance, before_payout.insurance);
                assert_eq!(group.vault + paid, supply);
                let claims = u128::try_from(values(&env, portfolios).iter().sum::<i128>()).unwrap();
                let residual = group.vault.checked_sub(claims + group.insurance).unwrap();
                println!("lane23 share={maintenance_share} collect_first={collect_first} shared={common_owner}: episodes={episodes:?}, charged={}, rebates={}, forgiven={}, payout={payout}, budgets={:?}, residual={residual}",
                    book.charged, book.rebates, book.forgiven, &group.insurance_domain_budget[..4]);
                let outcome = (
                    episodes,
                    values(&env, portfolios),
                    payout,
                    group.insurance,
                    group.vault,
                    group.insurance_domain_budget,
                    residual,
                    book,
                );
                if let Some(expected) = &reference {
                    assert_eq!(
                        &outcome, expected,
                        "common ownership preserves each portfolio's entitlement"
                    );
                } else {
                    reference = Some(outcome);
                }
            }
            order_outcomes.push(reference.unwrap());
        }
        assert_eq!(order_outcomes[0].0, order_outcomes[1].0);
        assert_eq!(order_outcomes[0].1, order_outcomes[1].1);
        assert!(
            order_outcomes[1].2 > order_outcomes[0].2,
            "clipped fees are forgiven at collection, so reward/collection order need not commute"
        );
        assert_eq!(
            order_outcomes[1].2 - order_outcomes[0].2,
            order_outcomes[0].3 - order_outcomes[1].3
        );
        assert_eq!(order_outcomes[0].5[2..], order_outcomes[1].5[2..]);
    }
    assert_eq!(liquidations, 16);
    assert_eq!(rollbacks, 72);
    assert_cu_within(
        "clipped maintenance / Hybrid reward transaction",
        peak,
        ACTION_CU,
    );
    println!("lane23: 8 histories, {liquidations} liquidations, 24 idempotent fee retries, {rollbacks} exact rollbacks, 8 exact SPL payouts; peak={peak} CU");
}
