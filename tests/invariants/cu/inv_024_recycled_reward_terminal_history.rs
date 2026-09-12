//! INV-024: a fee recipient can later pay fees and recycle a live SPL payout into
//! principal without changing any owner's history-derived terminal entitlement.
//!
//! Unlike the mixed-rail retry and trade/PnL histories, this composes a three-owner
//! reward cycle, partial Withdraw, Deposit from the same real ATA, and CloseResolved.
//! All six terminal orders preserve a fourth owner's untouched account until exit.
//! The ledger uses only public amounts, slots and policy, never observed deltas or
//! engine fee helpers. Scope: funded flat accounts, unclipped fixed-rate fees, one
//! mint; no trading, insolvency, alternate rails or rollback claim is made here.

use super::*;

#[derive(Clone, Copy, Default, Debug)]
struct OwnerQuoteHistory {
    deposits: u64,
    payouts: u64,
    fees: u64,
    rewards: u64,
    fee_slot: u64,
}

impl OwnerQuoteHistory {
    fn claim(self) -> u64 {
        (self.deposits + self.rewards)
            .checked_sub(self.fees + self.payouts)
            .expect("history must leave a nonnegative owner claim")
    }

    fn charge(&mut self, slot: u64, rate: u64) -> u64 {
        let charged = (slot - self.fee_slot) * rate;
        assert!(charged > 0 && charged < self.claim(), "unclipped fee");
        self.fees += charged;
        self.fee_slot = slot;
        charged
    }
}

#[test]
fn v16_program_recycled_rewards_preserve_owner_atoms_through_ordered_terminal_payouts() {
    use super::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    const INITIAL: [u64; 4] = [1_009, 2_003, 4_007, 8_009];
    const RATE: u64 = 7;
    const SHARE_BPS: u16 = 3_333;
    const RESOLVE_SLOT: u64 = 17;
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let supply: u64 = INITIAL.iter().sum();
    let mut max_cu = [0; 5]; // deposit, withdraw, reward sync, resolve, terminal payout
    let mut checked_routes = 0;
    for order in ORDERS {
        let context = format!("terminal order={order:?}");
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                maintenance_fee_per_slot: RATE.into(),
                ..V16CuMarketParams::default()
            },
        );
        env.update_maintenance_fee_policy_with_cu(SHARE_BPS);
        let owners = std::array::from_fn::<_, 4, _>(|_| Keypair::new());
        let portfolios = std::array::from_fn::<_, 4, _>(|actor| {
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
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolio.pubkey());
            portfolio.pubkey()
        });
        let tokens = std::array::from_fn::<_, 4, _>(|actor| {
            let token =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    INITIAL[actor],
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
        let mint_before = env.svm.get_account(&env.mint).unwrap();
        let mint = Mint::unpack(&mint_before.data).unwrap();
        assert_eq!(mint.supply, supply);
        assert_eq!(mint.mint_authority, COption::None);
        let owners_before = owners
            .each_ref()
            .map(|owner| env.svm.get_account(&owner.pubkey()));
        let deposit = |env: &mut V16CuEnv, actor: usize, amount: u64| {
            env.send(
                env.deposit_ix(portfolios[actor], amount.into()),
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
            .expect("public deposit from the owner's existing ATA")
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
        let check = |env: &V16CuEnv, history: &[OwnerQuoteHistory; 4]| {
            let mut capital = 0u64;
            let mut external = 0u64;
            for actor in 0..4 {
                let h = history[actor];
                let p = env.portfolio_state(portfolios[actor]);
                let token_account = env.svm.get_account(&tokens[actor]).unwrap();
                assert_eq!(token_account.owner, spl_token::ID);
                let token = TokenAccount::unpack(&token_account.data).unwrap();
                assert_eq!(token.owner, owners[actor].pubkey());
                assert_eq!(token.mint, env.mint);
                assert_eq!(
                    token.amount,
                    INITIAL[actor] + h.payouts - h.deposits,
                    "{context}: owner {actor} external atoms"
                );
                assert_eq!(
                    p.capital.get(),
                    u128::from(h.claim()),
                    "{context}: owner {actor} claim"
                );
                assert_eq!(p.last_fee_slot.get(), h.fee_slot);
                assert_eq!(p.pnl.get(), 0);
                assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                assert_eq!(
                    u128::from(token.amount) + p.capital.get() + u128::from(h.fees),
                    u128::from(INITIAL[actor] + h.rewards),
                    "{context}: owner {actor} attribution, not just aggregate conservation"
                );
                capital += h.claim();
                external += token.amount;
                assert_eq!(
                    env.svm.get_account(&owners[actor].pubkey()),
                    owners_before[actor]
                );
            }
            let insurance = history.iter().map(|h| h.fees).sum::<u64>()
                - history.iter().map(|h| h.rewards).sum::<u64>();
            let group = env.market_state().1;
            assert_eq!(group.c_tot, u128::from(capital));
            assert_eq!(group.insurance, u128::from(insurance));
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.vault, u128::from(capital + insurance));
            assert_eq!(env.token_amount(env.vault), capital + insurance);
            assert_eq!(external + env.token_amount(env.vault), supply);
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
            assert_domain_budget_remaining_total_consistent(&group, &context);
        };
        let mut history = [OwnerQuoteHistory::default(); 4];
        check(&env, &history);
        for actor in 0..4 {
            let cu = deposit(&mut env, actor, INITIAL[actor]);
            max_cu[0] = max_cu[0].max(cu);
            history[actor].deposits += INITIAL[actor];
            check(&env, &history);
            checked_routes += 1;
        }
        let sentinel = env.svm.get_account(&portfolios[3]);

        // Each credited keeper next pays its own fees, withdraws, and redeposits
        // part of that payout. No minting or synthetic token setup occurs here.
        for (slot, payer, recipient, withdrawn, redeposited) in
            [(5, 0, 1, 137, 53), (9, 1, 2, 211, 89), (13, 2, 0, 73, 31)]
        {
            env.svm.warp_to_slot(slot);
            let before = portfolios.map(|key| env.svm.get_account(&key));
            let custody_before = tokens
                .into_iter()
                .chain([env.vault])
                .map(|key| env.svm.get_account(&key))
                .collect::<Vec<_>>();
            let fee = history[payer].charge(slot, RATE);
            let reward = fee * u64::from(SHARE_BPS) / 10_000;
            assert!(reward > 0 && reward < fee);
            assert_ne!(fee * u64::from(SHARE_BPS) % 10_000, 0, "rounding residue");
            history[recipient].rewards += reward;
            let cu = env.sync_maintenance_fee_with_cu(
                portfolios[payer],
                Some(portfolios[recipient]),
                slot,
            );
            max_cu[2] = max_cu[2].max(cu);
            check(&env, &history);
            checked_routes += 1;
            assert_eq!(
                tokens
                    .into_iter()
                    .chain([env.vault])
                    .map(|key| env.svm.get_account(&key))
                    .collect::<Vec<_>>(),
                custody_before
            );
            for peer in 0..4 {
                if peer != payer && peer != recipient {
                    assert_eq!(env.svm.get_account(&portfolios[peer]), before[peer]);
                }
            }

            env.svm.warp_to_slot(slot + 1);
            let peers = portfolios.map(|key| env.svm.get_account(&key));
            history[recipient].charge(slot + 1, RATE);
            assert!(
                withdrawn < history[recipient].claim(),
                "strict partial payout"
            );
            let cu = env
                .send(
                    env.withdraw_ix(portfolios[recipient], withdrawn.into()),
                    payout_metas(&env, recipient),
                    &[&owners[recipient]],
                )
                .expect("live payout to the rewarded owner's ATA");
            max_cu[1] = max_cu[1].max(cu);
            history[recipient].payouts += withdrawn;
            check(&env, &history);
            checked_routes += 1;

            env.svm.warp_to_slot(slot + 2);
            assert!(redeposited > 0 && redeposited < withdrawn);
            let cu = deposit(&mut env, recipient, redeposited);
            max_cu[0] = max_cu[0].max(cu);
            history[recipient].deposits += redeposited;
            check(&env, &history);
            checked_routes += 1;
            for peer in 0..4 {
                if peer != recipient {
                    assert_eq!(env.svm.get_account(&portfolios[peer]), peers[peer]);
                }
            }
            assert_eq!(env.svm.get_account(&portfolios[3]), sentinel);
        }
        assert_eq!(history.map(|h| h.rewards), [6, 11, 6, 0]);
        assert_eq!(tokens.map(|key| env.token_amount(key)), [42, 84, 122, 0]);

        env.svm.warp_to_slot(RESOLVE_SLOT);
        let before_resolve = portfolios.map(|key| env.svm.get_account(&key));
        max_cu[3] = max_cu[3].max(env.resolve());
        assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        assert_eq!(env.market_state().1.resolved_slot, RESOLVE_SLOT);
        assert_eq!(
            portfolios.map(|key| env.svm.get_account(&key)),
            before_resolve
        );
        check(&env, &history);
        checked_routes += 1;

        // Fees stop at resolution even though all SPL payouts occur much later.
        env.svm.warp_to_slot(RESOLVE_SLOT + 100);
        for actor in order.into_iter().chain([3]) {
            let before = portfolios.map(|key| env.svm.get_account(&key));
            history[actor].charge(RESOLVE_SLOT, RATE);
            let payout = history[actor].claim();
            assert!(payout > 0);
            let cu = env
                .send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    payout_metas(&env, actor),
                    &[&owners[actor]],
                )
                .expect("one flat-account terminal close pays the history-derived remainder");
            max_cu[4] = max_cu[4].max(cu);
            history[actor].payouts += payout;
            check(&env, &history);
            checked_routes += 1;
            assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
            for peer in 0..4 {
                if peer != actor {
                    assert_eq!(env.svm.get_account(&portfolios[peer]), before[peer]);
                }
            }
        }
        assert_eq!(history.map(|h| h.fees), [RESOLVE_SLOT * RATE; 4]);
        assert_eq!(history.map(OwnerQuoteHistory::claim), [0; 4]);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [896, 1_895, 3_894, 7_890]
        );
        assert_eq!(
            env.token_amount(env.vault),
            453,
            "only attributed retained fees remain"
        );
    }
    assert_eq!(checked_routes, 6 * 18);
    for (label, cu) in [
        "deposit",
        "withdraw",
        "reward sync",
        "resolve",
        "terminal payout",
    ]
    .into_iter()
    .zip(max_cu)
    {
        assert_cu_within(label, cu, CUSTODY_CU_LIMIT);
    }
    eprintln!("INV-024 recycled rewards: 6 terminal orders, {checked_routes} checked routes; max CU [deposit, withdraw, sync, resolve, terminal]={max_cu:?}");
}
