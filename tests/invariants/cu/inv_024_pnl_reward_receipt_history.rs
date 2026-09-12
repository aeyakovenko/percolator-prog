//! INV-024: a paid PnL/reward recipient later loses, without resetting its claim.
//!
//! Public System/SPL/ATA setup, two signed trade rounds, rounded maintenance
//! rewards, conversion, payout/redeposit, backing expiry, and a funded receipt.
//! Expected amounts come from inputs and history, never observed value deltas.
//! Scope: solvent one-unit positions, unclipped fees, zero funding, one mint.

use super::*;

#[derive(Clone, Copy, Debug, Default)]
struct EpisodeClaim {
    principal: u128,
    gains: u128,
    losses: u128,
    fees: u128,
    trade_fees: u128,
    maintenance_domains: [u128; 2],
    rewards: u128,
    converted: u128,
    terminal_receipts: u128,
    paid: u128,
    fee_slot: u64,
}

impl EpisodeClaim {
    fn claim(self) -> u128 {
        // Receipting retires the same live face; it is not a second PnL award.
        let live_gains = self.gains.checked_sub(self.terminal_receipts).unwrap();
        (self.principal + live_gains + self.rewards + self.terminal_receipts)
            .checked_sub(self.losses + self.fees + self.paid)
            .expect("public history must fund this owner's remaining claim")
    }

    fn pnl(self) -> u128 {
        self.gains
            .checked_sub(self.converted + self.terminal_receipts)
            .unwrap()
    }

    fn charge(&mut self, slot: u64, rate: u128, share_bps: u16) -> u128 {
        let fee = u128::from(slot.checked_sub(self.fee_slot).unwrap()) * rate;
        assert!(fee < self.claim() - self.pnl(), "unclipped capital fee");
        self.fees += fee;
        let retained = fee - fee * u128::from(share_bps) / 10_000;
        self.maintenance_domains[0] += retained / 2;
        self.maintenance_domains[1] += retained - retained / 2;
        self.fee_slot = slot;
        fee
    }

    fn trade_fee(&mut self, fee: u128) {
        self.trade_fees += fee;
        self.fees += fee;
    }

    fn observation(self, endowment: u128) -> Option<(u128, i128, u128)> {
        let external = endowment
            .checked_add(self.paid)?
            .checked_sub(self.principal)?;
        Some((self.claim() - self.pnl(), self.pnl() as i128, external))
    }
}

fn observations_match(
    history: &[EpisodeClaim; 4],
    endowments: [u128; 4],
    observed: &[(u128, i128, u128); 4],
) -> bool {
    (0..4).all(|actor| Some(observed[actor]) == history[actor].observation(endowments[actor]))
}

#[test]
fn v16_program_pnl_rewards_and_receipts_preserve_history_wide_owner_claims() {
    use super::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    const INITIAL: [u128; 4] = [2_000_003, 3_000_007, 70_001, 50_003];
    const PRICES: [u64; 3] = [1_000_000, 1_100_001, 1_160_004];
    const SLOTS: [u64; 3] = [1, 5, 9];
    const RATE: u128 = 7;
    const REDEPOSIT: u128 = 17_003;
    const BACKING_LIFETIME: u64 = 100;
    let mut worlds = 0;
    let mut checked_routes = 0;
    let mut conversions = 0;
    let mut live_payouts = 0;
    let mut terminal_payouts = 0;
    let mut terminal_preparations = 0;
    let mut receipt_retries = 0;
    let mut max_cu = 0;
    let mut outcomes = [None; 2];

    for first_winner in 0..2 {
        for winner_first in [false, true] {
            for split in [false, true] {
                for reverse_exit in [false, true] {
                    let context = format!(
                        "first_winner={first_winner} winner_first={winner_first} split={split} reverse_exit={reverse_exit}"
                    );
                    let mut env = inv018_public_spl_market_with_params(
                        6,
                        V16CuMarketParams {
                            initial_price: PRICES[0],
                            maintenance_fee_per_slot: RATE,
                            max_bankrupt_close_lifetime_slots: BACKING_LIFETIME,
                            ..V16CuMarketParams::default()
                        },
                    );
                    env.svm.warp_to_slot(SLOTS[0]);
                    env.configure_auth_mark_with_cu(SLOTS[0], PRICES[0]);
                    let owners = std::array::from_fn::<_, 4, _>(|_| Keypair::new());
                    let portfolios = std::array::from_fn::<_, 4, _>(|actor| {
                        env.svm
                            .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                            .unwrap();
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
                                AccountMeta::new(owners[actor].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(key.pubkey(), false),
                            ],
                            &[&owners[actor]],
                        )
                        .unwrap();
                        env.portfolios.push(key.pubkey());
                        key.pubkey()
                    });
                    let tokens = std::array::from_fn::<_, 4, _>(|actor| {
                        let token = create_ata_for_test(
                            &mut env.svm,
                            &env.payer,
                            owners[actor].pubkey(),
                            env.mint,
                        );
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                &token,
                                &env.admin.pubkey(),
                                &[],
                                INITIAL[actor] as u64,
                            )
                            .unwrap(),
                            &[&env.admin],
                        )
                        .unwrap();
                        token
                    });
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
                    let mint_frame = env.svm.get_account(&env.mint).unwrap();
                    let mint = Mint::unpack(&mint_frame.data).unwrap();
                    assert_eq!(u128::from(mint.supply), INITIAL.iter().sum::<u128>());
                    assert_eq!(mint.mint_authority, COption::None);
                    let ids = portfolios.map(|key| env.portfolio_id(key));
                    let owner_frame = owners
                        .each_ref()
                        .map(|key| env.svm.get_account(&key.pubkey()));
                    let token_frame = tokens
                        .into_iter()
                        .chain([env.vault])
                        .map(|key| (key, env.svm.get_account(&key).unwrap()))
                        .collect::<Vec<_>>();
                    let mut history = [EpisodeClaim {
                        fee_slot: SLOTS[0],
                        ..EpisodeClaim::default()
                    }; 4];
                    let observe = |env: &V16CuEnv| {
                        std::array::from_fn(|actor| {
                            let p = env.portfolio_state(portfolios[actor]);
                            (
                                p.capital.get(),
                                p.pnl.get(),
                                u128::from(env.token_amount(tokens[actor])),
                            )
                        })
                    };
                    let check = |env: &V16CuEnv, history: &[EpisodeClaim; 4], label: &str| {
                        let observed = observe(env);
                        assert!(
                            observations_match(history, INITIAL, &observed),
                            "{context} {label}: observed={observed:?} history={history:?}"
                        );
                        for actor in 0..4 {
                            let p = env.portfolio_state(portfolios[actor]);
                            assert_eq!(
                                p.last_fee_slot.get(),
                                history[actor].fee_slot,
                                "{context} {label} fee cursor {actor}"
                            );
                            assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                            let data = env.svm.get_account(&portfolios[actor]).unwrap().data;
                            let (header, owner) =
                                state::read_portfolio_owner_preflight(&data).unwrap();
                            assert_eq!(owner, owners[actor].pubkey().to_bytes());
                            assert_eq!(header.market_group_id, env.market.to_bytes());
                            assert_eq!(
                                env.svm.get_account(&owners[actor].pubkey()),
                                owner_frame[actor]
                            );
                        }
                        let (_, group) = env.market_state();
                        let vault = history.iter().map(|h| h.principal).sum::<u128>()
                            - history.iter().map(|h| h.paid).sum::<u128>();
                        let capital = history.iter().map(|h| h.claim() - h.pnl()).sum::<u128>();
                        let insurance = history.iter().map(|h| h.fees).sum::<u128>()
                            - history.iter().map(|h| h.rewards).sum::<u128>();
                        assert_eq!(
                            (group.vault, group.c_tot, group.insurance),
                            (vault, capital, insurance),
                            "{context} {label}"
                        );
                        let side_trade_fees =
                            history.iter().map(|h| h.trade_fees).sum::<u128>() / 2;
                        for (domain, budget) in group.insurance_domain_budget.iter().enumerate() {
                            assert_eq!(
                                *budget,
                                match domain {
                                    0 | 1 =>
                                        side_trade_fees
                                            + history
                                                .iter()
                                                .map(|h| h.maintenance_domains[domain])
                                                .sum::<u128>(),
                                    _ => 0,
                                },
                                "{context} {label} fee domain {domain}"
                            );
                        }
                        assert_eq!(u128::from(env.token_amount(env.vault)), vault);
                        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                        assert_eq!(
                            vault + observed.iter().map(|value| value.2).sum::<u128>(),
                            INITIAL.iter().sum::<u128>()
                        );
                        for (index, (key, before)) in token_frame.iter().enumerate() {
                            let after = env.svm.get_account(key).unwrap();
                            let mut expected = TokenAccount::unpack(&before.data).unwrap();
                            expected.amount = if index < 4 {
                                history[index].observation(INITIAL[index]).unwrap().2 as u64
                            } else {
                                vault as u64
                            };
                            assert_eq!(TokenAccount::unpack(&after.data).unwrap(), expected);
                            assert_eq!(
                                (
                                    after.owner,
                                    after.lamports,
                                    after.executable,
                                    after.rent_epoch
                                ),
                                (
                                    before.owner,
                                    before.lamports,
                                    before.executable,
                                    before.rent_epoch
                                )
                            );
                        }
                    };
                    let deposit = |env: &mut V16CuEnv, actor: usize, amount| {
                        env.send(
                            env.deposit_ix(portfolios[actor], amount),
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
                        .expect("public deposit")
                    };
                    let payout_metas = |env: &V16CuEnv, actor: usize| {
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ]
                    };
                    macro_rules! step {
                        ($label:expr, $mutable:expr, $call:expr, $update:block) => {{
                            let before = portfolios.map(|key| env.svm.get_account(&key));
                            env.svm.expire_blockhash();
                            let cu = $call;
                            $update
                            check(&env, &history, $label);
                            for actor in 0..4 {
                                if !$mutable.contains(&actor) {
                                    assert_eq!(env.svm.get_account(&portfolios[actor]), before[actor], "{context} {} peer {actor}", $label);
                                }
                            }
                            assert_cu_within($label, cu, TRADE_CU_LIMIT);
                            max_cu = max_cu.max(cu);
                            checked_routes += 1;
                        }};
                    }
                    check(&env, &history, "empty public setup");
                    for actor in 0..4 {
                        step!(
                            "initial deposit",
                            [actor],
                            deposit(&mut env, actor, INITIAL[actor]),
                            {
                                history[actor].principal += INITIAL[actor];
                            }
                        );
                    }
                    let initial_epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
                    for round in 0..2 {
                        let winner = first_winner ^ round;
                        let loser = 1 - winner;
                        let size = if winner == 0 {
                            POS_SCALE as i128
                        } else {
                            -(POS_SCALE as i128)
                        };
                        let gain = u128::from(PRICES[round + 1] - PRICES[round]);
                        let fee_bps = [7_u64, 13][round];
                        let share = [3_333_u16, 6_667][round];
                        step!(
                            "reward policy",
                            [],
                            env.update_maintenance_fee_policy_with_cu(share),
                            {}
                        );
                        let open_fee =
                            (u128::from(PRICES[round]) * u128::from(fee_bps)).div_ceil(10_000);
                        step!(
                            "round open",
                            [0, 1],
                            env.trade_with_cu(
                                &owners[0],
                                portfolios[0],
                                &owners[1],
                                portfolios[1],
                                size,
                                PRICES[round],
                                fee_bps,
                            ),
                            {
                                history[0].trade_fee(open_fee);
                                history[1].trade_fee(open_fee);
                            }
                        );
                        if round == 1 {
                            for actor in 0..2 {
                                assert_ne!(
                                    env.portfolio_position_epoch(portfolios[actor]),
                                    initial_epochs[actor]
                                );
                            }
                        }
                        let slot = SLOTS[round + 1];
                        env.svm.warp_to_slot(slot);
                        step!(
                            "authenticated mark",
                            [],
                            env.push_auth_mark_with_cu(slot, PRICES[round + 1]),
                            {}
                        );
                        for _ in 0..5 {
                            if env.market_state().1.assets[0].slot_last == slot {
                                break;
                            }
                            step!(
                                "market catchup",
                                [2],
                                env.send(
                                    ProgInstruction::PermissionlessCrank {
                                        now_slot: slot,
                                        observations: crank_observations(0)
                                    },
                                    vec![
                                        AccountMeta::new(env.payer.pubkey(), true),
                                        AccountMeta::new(env.market, false),
                                        AccountMeta::new(portfolios[2], false)
                                    ],
                                    &[],
                                )
                                .expect("single public market catchup"),
                                {}
                            );
                        }
                        assert_eq!(env.market_state().1.assets[0].slot_last, slot);
                        let order = if winner_first {
                            [winner, loser]
                        } else {
                            [loser, winner]
                        };
                        for actor in order {
                            step!(
                                "account settlement",
                                [actor],
                                env.send(
                                    ProgInstruction::PermissionlessCrank {
                                        now_slot: slot,
                                        observations: vec![]
                                    },
                                    vec![
                                        AccountMeta::new(env.payer.pubkey(), true),
                                        AccountMeta::new(env.market, false),
                                        AccountMeta::new(portfolios[actor], false)
                                    ],
                                    &[],
                                )
                                .expect("single public account settlement"),
                                {
                                    if actor == winner {
                                        history[actor].gains += gain;
                                    } else {
                                        history[actor].losses += gain;
                                    }
                                    assert!(history[actor].charge(slot, RATE, 0) > 0);
                                }
                            );
                        }
                        let close_fee =
                            (u128::from(PRICES[round + 1]) * u128::from(fee_bps)).div_ceil(10_000);
                        step!(
                            "round close without recharging elapsed fees",
                            [0, 1],
                            env.trade_with_cu(
                                &owners[0],
                                portfolios[0],
                                &owners[1],
                                portfolios[1],
                                -size,
                                PRICES[round + 1],
                                fee_bps,
                            ),
                            {
                                for h in &mut history[..2] {
                                    assert_eq!(h.charge(slot, RATE, 0), 0);
                                    h.trade_fee(close_fee);
                                }
                            }
                        );
                        for actor in 0..2 {
                            assert!(percolator::active_bitmap_is_empty(active_bitmap(
                                &env.portfolio_state(portfolios[actor])
                            )));
                        }
                        let fee = u128::from(slot - history[2].fee_slot) * RATE;
                        let reward = fee * u128::from(share) / 10_000;
                        assert!(reward > 0 && reward < fee);
                        assert_ne!(fee * u128::from(share) % 10_000, 0);
                        step!(
                            "reward beside unconverted PnL",
                            [2, winner],
                            env.sync_maintenance_fee_with_cu(
                                portfolios[2],
                                Some(portfolios[winner]),
                                slot,
                            ),
                            {
                                assert_eq!(history[2].charge(slot, RATE, share), fee);
                                history[winner].rewards += reward;
                            }
                        );
                        step!(
                            "refresh rewarded claim",
                            [winner],
                            env.send(
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: slot,
                                    observations: crank_observations(0),
                                },
                                vec![
                                    AccountMeta::new(env.payer.pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(portfolios[winner], false),
                                ],
                                &[],
                            )
                            .expect("public recertification after the reward credit"),
                            {}
                        );
                        if round == 0 {
                            assert!(gain <= history[winner].claim());
                            assert_eq!(gain, history[winner].pnl());
                            step!(
                                "live conversion",
                                [winner],
                                env.convert_released_pnl_with_cu(
                                    &owners[winner],
                                    portfolios[winner],
                                    gain,
                                ),
                                {
                                    history[winner].converted += gain;
                                }
                            );
                            conversions += 1;
                            let total = gain + reward;
                            let parts = if split {
                                vec![total / 3, total - total / 3]
                            } else {
                                vec![total]
                            };
                            for amount in parts {
                                assert!(amount > 0 && amount <= history[winner].claim());
                                assert!(amount <= history[winner].claim() - history[winner].pnl());
                                step!(
                                    "live payout",
                                    [winner],
                                    env.send(
                                        env.withdraw_ix(portfolios[winner], amount),
                                        payout_metas(&env, winner),
                                        &[&owners[winner]],
                                    )
                                    .expect("history-bounded live payout"),
                                    {
                                        history[winner].paid += amount;
                                    }
                                );
                                live_payouts += 1;
                            }
                            assert!(REDEPOSIT < total);
                            step!(
                                "redeposit paid PnL and reward",
                                [winner],
                                deposit(&mut env, winner, REDEPOSIT),
                                {
                                    history[winner].principal += REDEPOSIT;
                                }
                            );
                            let observed = observe(&env);
                            let mut wrong_owner = observed;
                            wrong_owner[winner].0 -= 1;
                            wrong_owner[loser].0 += 1;
                            assert!(!observations_match(&history, INITIAL, &wrong_owner));
                            for omitted in [1, history[winner].paid] {
                                let mut forgotten_payout = history;
                                forgotten_payout[winner].paid -= omitted;
                                assert!(!observations_match(&forgotten_payout, INITIAL, &observed));
                            }
                        }
                    }
                    let final_winner = 1 - first_winner;
                    let face = u128::from(PRICES[2] - PRICES[1]);
                    assert_eq!(history[final_winner].pnl(), face);
                    assert!(history[first_winner].paid > 0 && history[first_winner].losses > 0);
                    step!(
                        "settle senior maintenance before resolution",
                        [3],
                        env.sync_maintenance_fee_with_cu(portfolios[3], None, SLOTS[2]),
                        {
                            assert!(history[3].charge(SLOTS[2], RATE, 0) > 0);
                        }
                    );
                    // Make the fee donor and senior current before comparing payout
                    // orders; fee collection invalidated their certificates.
                    for actor in [2, 3] {
                        step!(
                            "certify terminal senior",
                            [actor],
                            env.send(
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: SLOTS[2],
                                    observations: crank_observations(0),
                                },
                                vec![
                                    AccountMeta::new(env.payer.pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(portfolios[actor], false),
                                ],
                                &[],
                            )
                            .expect("public senior recertification"),
                            {}
                        );
                    }
                    let group = env.market_state().1;
                    assert_eq!(group.vault - group.c_tot - group.insurance, face);
                    assert_eq!(group.source_claim_bound_total_num, face * BOUND_SCALE);
                    let admin = Keypair::from_bytes(&env.admin.to_bytes()).unwrap();
                    step!(
                        "resolve composed history",
                        [],
                        env.send(
                            ProgInstruction::ResolveMarket {
                                asset_generation_frontier: 0,
                                authority_epoch: env.control_sequences(0).authority_epoch
                            },
                            vec![
                                AccountMeta::new(admin.pubkey(), true),
                                AccountMeta::new(env.market, false)
                            ],
                            &[&admin],
                        )
                        .expect("single public resolution"),
                        {}
                    );
                    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
                    assert_eq!(env.market_state().1.resolved_slot, SLOTS[2]);
                    env.svm.warp_to_slot(SLOTS[2] + BACKING_LIFETIME);
                    assert_eq!(
                        env.market_state()
                            .1
                            .source_backing_buckets
                            .iter()
                            .filter(|b| b.status == BackingBucketStatusV16::Fresh
                                && b.expiry_slot == SLOTS[2] + BACKING_LIFETIME)
                            .count(),
                        1,
                        "history reaches exactly one backing-expiry boundary"
                    );
                    let order = if reverse_exit {
                        [3, 2, 1, 0]
                    } else {
                        [0, 1, 2, 3]
                    };
                    for actor in order {
                        if actor == final_winner {
                            step!(
                                "terminal expiry preparation pays nothing",
                                [actor],
                                env.send(
                                    ProgInstruction::CloseResolved {
                                        fee_rate_per_slot: 0
                                    },
                                    payout_metas(&env, actor),
                                    &[&owners[actor]],
                                )
                                .expect("one bounded expiry normalization"),
                                {}
                            );
                            assert!(
                                env.market_state()
                                    .1
                                    .source_backing_buckets
                                    .iter()
                                    .all(|b| b.status != BackingBucketStatusV16::Fresh
                                        || b.expiry_slot > SLOTS[2] + BACKING_LIFETIME),
                                "expiry preparation must make structural progress"
                            );
                            terminal_preparations += 1;
                        }
                        let mut next = history[actor];
                        next.charge(SLOTS[2], RATE, 0);
                        let due = next.claim();
                        let receipt = next.pnl();
                        assert!(due > 0 && due <= history[actor].claim());
                        next.terminal_receipts += receipt;
                        assert_eq!(next.claim(), due, "receipting must not enlarge entitlement");
                        next.paid += due;
                        step!(
                            "terminal owner payout",
                            [actor],
                            env.send(
                                ProgInstruction::CloseResolved {
                                    fee_rate_per_slot: 0
                                },
                                payout_metas(&env, actor),
                                &[&owners[actor]],
                            )
                            .expect("history-bounded terminal payout"),
                            {
                                history[actor] = next;
                            }
                        );
                        terminal_payouts += 1;
                        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
                        if receipt > 0 {
                            let r = env
                                .portfolio_state(portfolios[actor])
                                .resolved_payout_receipt
                                .try_to_runtime()
                                .unwrap();
                            assert!(r.present && r.finalized);
                            assert_eq!(
                                (
                                    r.terminal_positive_claim_face,
                                    r.paid_effective,
                                    r.prior_bound_contribution_num
                                ),
                                (face, face, face * BOUND_SCALE)
                            );
                            assert_eq!(r.live_released_face_at_receipt, 0);
                            let before = env.svm.get_account(&env.market);
                            assert_eq!(history[actor].claim(), 0);
                            step!(
                                "paid receipt retry",
                                [],
                                env.send(
                                    ProgInstruction::ClaimResolvedPayoutTopup,
                                    payout_metas(&env, actor),
                                    &[&owners[actor]],
                                )
                                .expect("fresh paid-receipt retry"),
                                {}
                            );
                            assert_eq!(env.svm.get_account(&env.market), before);
                            receipt_retries += 1;
                        }
                    }
                    assert_eq!(history.map(EpisodeClaim::claim), [0; 4]);
                    assert_eq!(
                        history.map(|h| h.terminal_receipts).iter().sum::<u128>(),
                        face
                    );
                    assert_eq!(history[2].fees, u128::from(SLOTS[2] - SLOTS[0]) * RATE);
                    assert_eq!(history[first_winner].rewards, 9);
                    assert_eq!(history[final_winner].rewards, 18);
                    assert_eq!(env.market_state().1.source_claim_bound_total_num, 0);
                    let observed = observe(&env);
                    let mut forgotten_loss = history;
                    forgotten_loss[first_winner].losses -= 1;
                    assert!(!observations_match(&forgotten_loss, INITIAL, &observed));
                    let mut duplicate_receipt = observed;
                    duplicate_receipt[final_winner].2 += face;
                    duplicate_receipt[2].2 -= face;
                    assert!(!observations_match(&history, INITIAL, &duplicate_receipt));
                    let paid = tokens.map(|key| env.token_amount(key));
                    assert_eq!(*outcomes[first_winner].get_or_insert(paid), paid,
                        "settlement order, payout partition, or terminal order changed final owner wealth");
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (
            worlds,
            conversions,
            live_payouts,
            terminal_payouts,
            terminal_preparations,
            receipt_retries
        ),
        (16, 16, 24, 64, 16, 16)
    );
    assert_eq!(checked_routes, 664);
    eprintln!("INV-024 composed PnL/reward/receipt history: {worlds} worlds, {checked_routes} checked routes, {conversions} conversions, {live_payouts} live payouts, {terminal_payouts} terminal payouts, {terminal_preparations} expiry preparations, {receipt_retries} receipt retries; max CU {max_cu}");
}
