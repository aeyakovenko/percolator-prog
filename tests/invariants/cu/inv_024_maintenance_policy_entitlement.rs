//! INV-024: identical aggregate fees do not imply identical owner entitlements.
//!
//! Two retained public fee-sync instructions straddle a share-policy update in
//! opposite orders, following identical public setup histories. Equal gross
//! fees leave equal insurance and total keeper rewards, but different keepers
//! own the rounding-sensitive credits. Later live withdrawals collect each
//! owner's own elapsed fees and pay only that owner's history-derived claim.
//! No resolution, reward recycling, synthetic account bytes, or engine fee oracle.
//! Scope: one mint, flat funded portfolios, unclipped fixed-rate maintenance fees.

use super::*;

#[derive(Clone, Copy, Debug, Default)]
struct MaintenanceEntitlement {
    deposited: u64,
    paid: u64,
    fees: u64,
    rewards: u64,
    fee_slot: u64,
}

impl MaintenanceEntitlement {
    fn claim(self) -> u64 {
        (self.deposited + self.rewards)
            .checked_sub(self.paid + self.fees)
            .expect("owner's public history must fund the claim")
    }

    fn charge(&mut self, slot: u64, rate: u64) -> u64 {
        let fee = (slot - self.fee_slot) * rate;
        assert!(fee > 0 && fee < self.claim(), "positive unclipped fee");
        self.fees += fee;
        self.fee_slot = slot;
        fee
    }
}

#[test]
fn v16_program_maintenance_policy_interleaving_preserves_each_owners_live_entitlement() {
    use super::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    const INITIAL: [u64; 4] = [1_009, 2_003, 4_007, 8_009];
    const RATE: u64 = 7;
    const SYNC_SLOT: u64 = 5;
    const SHARES: [u16; 2] = [3_333, 6_667];
    let supply: u64 = INITIAL.iter().sum();
    let owners = std::array::from_fn::<_, 4, _>(|_| Keypair::new());
    let mut outcomes = Vec::new();
    let mut max_cu = [0; 3]; // sync, policy, withdraw
    for order in [[0, 1], [1, 0]] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                maintenance_fee_per_slot: RATE.into(),
                ..V16CuMarketParams::default()
            },
        );
        env.update_maintenance_fee_policy_with_cu(SHARES[0]);
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
        let portfolio_frame = |env: &V16CuEnv| portfolios.map(|key| env.svm.get_account(&key));
        let custody_frame = |env: &V16CuEnv| {
            tokens
                .into_iter()
                .chain([env.vault])
                .map(|key| env.svm.get_account(&key))
                .collect::<Vec<_>>()
        };
        let check = |env: &V16CuEnv, ledger: &[MaintenanceEntitlement; 4], label: &str| {
            let mut capital = 0;
            let mut external = 0;
            for actor in 0..4 {
                let h = ledger[actor];
                let p = env.portfolio_state(portfolios[actor]);
                let token_account = env.svm.get_account(&tokens[actor]).unwrap();
                assert_eq!(token_account.owner, spl_token::ID);
                let token = TokenAccount::unpack(&token_account.data).unwrap();
                assert_eq!(token.owner, owners[actor].pubkey());
                assert_eq!(token.mint, env.mint);
                assert_eq!(
                    token.amount,
                    INITIAL[actor] + h.paid - h.deposited,
                    "{label}: owner {actor} SPL atoms"
                );
                assert_eq!(
                    p.capital.get(),
                    u128::from(h.claim()),
                    "{label}: owner {actor}"
                );
                assert_eq!(
                    p.last_fee_slot.get(),
                    h.fee_slot,
                    "{label}: owner {actor} fee cursor"
                );
                assert_eq!(p.pnl.get(), 0);
                assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                assert_eq!(
                    u128::from(token.amount + h.fees) + p.capital.get(),
                    u128::from(INITIAL[actor] + h.rewards),
                    "{label}: every owner's atoms reconcile independently"
                );
                assert_eq!(
                    env.svm.get_account(&owners[actor].pubkey()),
                    owners_before[actor]
                );
                capital += h.claim();
                external += token.amount;
            }
            let insurance = ledger.iter().map(|h| h.fees).sum::<u64>()
                - ledger.iter().map(|h| h.rewards).sum::<u64>();
            let group = env.market_state().1;
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.c_tot, u128::from(capital), "{label}");
            assert_eq!(group.insurance, u128::from(insurance), "{label}");
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.vault, u128::from(capital + insurance));
            assert_eq!(env.token_amount(env.vault), capital + insurance);
            assert_eq!(external + env.token_amount(env.vault), supply);
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
            assert_domain_budget_remaining_total_consistent(&group, label);
        };

        let mut initial_ledger = [MaintenanceEntitlement::default(); 4];
        check(&env, &initial_ledger, "initialized");
        for actor in 0..4 {
            env.send(
                env.deposit_ix(portfolios[actor], INITIAL[actor].into()),
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
            .unwrap();
            initial_ledger[actor].deposited = INITIAL[actor];
            check(&env, &initial_ledger, "deposit");
        }

        // Both syncs are constructed under the old policy. This permissionless route
        // uses policy at execution, not construction; the payer/keeper pairing stays fixed.
        let syncs = std::array::from_fn::<_, 2, _>(|payer| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[payer], false),
                AccountMeta::new(portfolios[payer + 2], false),
            ],
            data: ProgInstruction::SyncMaintenanceFee {
                now_slot: SYNC_SLOT,
            }
            .encode(),
        });
        let sequences = env.control_sequences(0);
        let policy = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            data: ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps: SHARES[1],
                policy_sequence: next_control_sequence(sequences.maintenance_fee),
                authority_epoch: sequences.authority_epoch,
            }
            .encode(),
        };
        let label = format!("policy interleaving {order:?}");
        let mut ledger = initial_ledger;
        env.svm.warp_to_slot(SYNC_SLOT);
        for (step, payer) in order.into_iter().enumerate() {
            if step == 1 {
                let portfolios_before = portfolio_frame(&env);
                let custody_before = custody_frame(&env);
                let cu = send_raw_tx(&mut env.svm, &env.payer, policy.clone(), &[&env.admin])
                    .expect("public share update between the two retained syncs");
                max_cu[1] = max_cu[1].max(cu);
                assert_eq!(portfolio_frame(&env), portfolios_before);
                assert_eq!(custody_frame(&env), custody_before);
                assert_eq!(
                    env.control_sequences(0).maintenance_fee,
                    sequences.maintenance_fee + 1
                );
                check(&env, &ledger, &label);
            }
            assert_eq!(
                env.market_state().0.maintenance_cranker_fee_share_bps,
                SHARES[step]
            );
            let before = portfolio_frame(&env);
            let custody_before = custody_frame(&env);
            let gross = ledger[payer].charge(SYNC_SLOT, RATE);
            let numerator = gross * u64::from(SHARES[step]);
            assert_ne!(numerator % 10_000, 0, "exercise floor residue");
            ledger[payer + 2].rewards += numerator / 10_000;
            let cu = send_raw_tx(&mut env.svm, &env.payer, syncs[payer].clone(), &[])
                .expect("retained permissionless sync uses the currently active share");
            max_cu[0] = max_cu[0].max(cu);
            check(&env, &ledger, &label);
            assert_eq!(custody_frame(&env), custody_before);
            for other in 0..4 {
                if other != payer && other != payer + 2 {
                    assert_eq!(env.svm.get_account(&portfolios[other]), before[other]);
                }
            }
        }
        let after_sync = env.market_state().1;
        assert_eq!(after_sync.insurance, 36);
        assert_eq!(after_sync.c_tot, u128::from(supply - 36));
        assert_eq!(ledger[order[0] + 2].rewards, 11);
        assert_eq!(ledger[order[1] + 2].rewards, 23);

        // Withdraw itself collects fees without a keeper reward. In particular,
        // receiving a reward must not advance the recipient's own fee cursor.
        for (slot, partial) in [(6, true), (8, false)] {
            env.svm.warp_to_slot(slot);
            for actor in [2, 0, 3, 1] {
                let before = portfolio_frame(&env);
                ledger[actor].charge(slot, RATE);
                let amount = if partial {
                    [137, 53, 97, 211][actor]
                } else {
                    ledger[actor].claim()
                };
                assert!(amount > 0 && amount <= ledger[actor].claim());
                let cu = env
                    .send(
                        env.withdraw_ix(portfolios[actor], amount.into()),
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[actor]],
                    )
                    .expect("public live withdrawal of the independently computed post-fee claim");
                max_cu[2] = max_cu[2].max(cu);
                ledger[actor].paid += amount;
                check(&env, &ledger, &label);
                for other in 0..4 {
                    if other != actor {
                        assert_eq!(env.svm.get_account(&portfolios[other]), before[other]);
                    }
                }
            }
        }
        assert!(ledger.iter().all(|h| h.claim() == 0 && h.fees == 56));
        let paid = tokens.map(|token| env.token_amount(token));
        let group = env.market_state().1;
        assert_eq!((group.c_tot, group.insurance, group.vault), (0, 190, 190));
        // The two 7-atom withdrawal fees each leave their odd atom on the short side.
        assert_eq!(group.insurance_domain_budget, vec![94, 96]);
        outcomes.push((
            paid,
            group.insurance_domain_budget,
            env.token_amount(env.vault),
        ));
    }

    // An aggregate-only oracle accepts swapping the keepers' 12-atom difference.
    // Literal independently calculated payouts pin which owner actually earned it.
    assert_eq!(outcomes[0].0, [953, 1_947, 3_962, 7_976]);
    assert_eq!(outcomes[1].0, [953, 1_947, 3_974, 7_964]);
    assert_eq!(
        outcomes[0].0.iter().sum::<u64>(),
        outcomes[1].0.iter().sum::<u64>()
    );
    assert_eq!(outcomes[0].1, outcomes[1].1);
    assert_eq!(outcomes[0].2, outcomes[1].2);
    for (route, cu) in ["sync", "update", "withdraw"].into_iter().zip(max_cu) {
        assert_cu_within(route, cu, CUSTODY_CU_LIMIT);
        eprintln!("INV-024 maintenance policy {route}: max {cu} CU");
    }
}
