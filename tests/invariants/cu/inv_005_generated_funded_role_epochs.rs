//! INV-005/020/024/027/055/081: interleaved funded role round trips preserve
//! unpaid entitlement, current observation scope, and independently held roles.

use super::super::{handoff, land, payout, profile, set_holder, signed};
use super::{fund_fixture, BACKING, CAPITAL, INSURANCE, SUPPLY};
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::*;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

// Each role occurs twice: first A -> B, then B -> A. This exhausts the
// interleavings while leaving the two transitions of each role ordered.
fn role_words(prefix: Vec<usize>, counts: [u8; 3], words: &mut Vec<Vec<usize>>) {
    if prefix.len() == 6 {
        words.push(prefix);
        return;
    }
    for role in 0..3 {
        if counts[role] < 2 {
            let mut next = prefix.clone();
            next.push(role);
            let mut counts = counts;
            counts[role] += 1;
            role_words(next, counts, words);
        }
    }
}

struct Book {
    backing: [u128; 4],
    insurance: [u128; 4],
    paid: [u128; 6],
    capital: u128,
    profiles: [state::AssetOracleProfileV16; 2],
    sequences: [state::AssetControlSequencesV16; 2],
}

impl Book {
    fn pay(&mut self, domain: usize, backing: bool, recipient: usize, amount: u128) {
        if backing {
            self.backing[domain] -= amount;
        } else {
            let long = amount.min(self.insurance[domain]);
            self.insurance[domain] -= long;
            self.insurance[domain + 1] -= amount - long;
            self.sequences[domain / 2].authority_epoch += 1;
        }
        self.paid[recipient] += amount;
    }

    fn check(&self, env: &V16CuEnv, wallets: &[Pubkey; 6], portfolio: Pubkey) {
        let (_, group) = env.market_state();
        let insurance = self.insurance.iter().sum::<u128>();
        let vault = self.capital + insurance + self.backing.iter().sum::<u128>();
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.c_tot, self.capital);
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.insurance, insurance);
        assert_eq!(group.vault, vault);
        assert_eq!(env.token_amount(env.vault) as u128, vault);
        assert_eq!(&group.insurance_domain_budget[..4], &self.insurance);
        assert!(group.insurance_domain_budget[4..].iter().all(|v| *v == 0));
        for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
            let amount = self.backing.get(domain).copied().unwrap_or(0);
            assert_eq!(bucket.fresh_unliened_backing_num, amount * BOUND_SCALE);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(bucket.utilization_fee_earnings, 0);
        }
        for asset in 0..2 {
            assert_eq!(group.assets[asset].lifecycle, AssetLifecycleV16::Active);
            assert_eq!(group.assets[asset].oi_eff_long_q, 0);
            assert_eq!(group.assets[asset].oi_eff_short_q, 0);
            assert_eq!(profile(env, asset), self.profiles[asset]);
            assert_eq!(env.control_sequences(asset), self.sequences[asset]);
        }
        let owner = env.portfolio_state(portfolio);
        assert_eq!(owner.capital.get(), self.capital);
        assert_eq!(owner.pnl.get(), 0);
        assert_eq!(wallets.map(|key| env.token_amount(key) as u128), self.paid);
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply as u128, SUPPLY);
        assert_eq!(vault + self.paid.iter().sum::<u128>(), SUPPLY);
    }
}

fn observe(env: &V16CuEnv, asset: usize, holder: Pubkey, epoch: u64, sequence: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(holder, true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::PushAuthMark {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: epoch,
            observation_sequence: sequence,
            now_slot: u64::MAX,
            mark_e6: 100,
        }
        .encode(),
    }
}

#[test]
fn v16_program_generated_funded_role_round_trips_preserve_entitlements_and_observation_scope() {
    let mut words = Vec::new();
    role_words(Vec::new(), [0; 3], &mut words);
    assert_eq!(words.len(), 90);
    assert_eq!(
        words
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        90
    );
    let mut rng = XorShiftRng::seed_from_u64(0x005_2026_0913);
    let mut worlds = 0;
    let mut transactions = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    for (word_index, word) in words.iter().enumerate() {
        let slices: [u128; 6] = std::array::from_fn(|_| rng.gen_range(1..4));
        for asset in 0..2 {
            for insurance_policy in [false, true] {
                let peer = 1 - asset;
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(1);
                let admin = env.admin.insecure_clone();
                let keys: [Keypair; 6] = std::array::from_fn(|_| Keypair::new());
                // Insurer, operator, backer, successor, cold admin, user.
                let actors = [&keys[0], &keys[1], &keys[2], &keys[3], &admin, &keys[4]];
                let interim = &keys[5];
                env.ensure_signer_account(interim.pubkey());
                let oracle_owner = if insurance_policy { 0 } else { 2 };
                let insurance_owner = if insurance_policy { 0 } else { 1 };
                let kinds = [
                    processor::ASSET_AUTH_ORACLE,
                    processor::ASSET_AUTH_BACKING_BUCKET,
                    if insurance_policy {
                        processor::ASSET_AUTH_INSURANCE
                    } else {
                        processor::ASSET_AUTH_INSURANCE_OPERATOR
                    },
                ];
                let originals = [oracle_owner, 2, insurance_owner];
                let mut holders = originals;
                let (wallets, portfolio) = fund_fixture(&mut env, &actors);
                for target in 0..2 {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(actors[oracle_owner]),
                        target,
                        processor::ASSET_AUTH_ORACLE,
                        actors[oracle_owner].pubkey().to_bytes(),
                    )
                    .unwrap();
                    env.configure_auth_mark_for_asset_with_authority(
                        target,
                        actors[oracle_owner],
                        1,
                        100,
                    );
                }
                let mut tracked = vec![
                    env.market,
                    env.mint,
                    env.vault,
                    env.vault_authority,
                    portfolio,
                    interim.pubkey(),
                ];
                tracked.extend(wallets);
                tracked.extend(actors.map(Signer::pubkey));
                let market = env.market;
                let vault = env.vault;
                let mut book = Book {
                    backing: BACKING,
                    insurance: INSURANCE,
                    paid: [0; 6],
                    capital: CAPITAL,
                    profiles: [profile(&env, 0), profile(&env, 1)],
                    sequences: [env.control_sequences(0), env.control_sequences(1)],
                };
                book.check(&env, &wallets, portfolio);
                let initial_epoch = book.sequences[asset].authority_epoch;
                let peer_economy = env.market_state().1.assets[peer];
                let peer_payout = payout(&env, actors[2].pubkey(), wallets[2], peer * 2, true, 1);
                let retained_peer = signed(&env, &[peer_payout.clone()], &[actors[2]]);
                let retained = (0..3)
                    .map(|role| {
                        let ix = handoff(
                            &env,
                            asset,
                            kinds[role],
                            actors[originals[role]].pubkey(),
                            Some(actors[3].pubkey()),
                            initial_epoch,
                        );
                        let mut signers = vec![actors[2], actors[3]];
                        if originals[role] != 2 {
                            signers.push(actors[originals[role]]);
                        }
                        signed(&env, &[peer_payout.clone(), ix], &signers)
                    })
                    .collect::<Vec<_>>();

                let mut run = |env: &mut V16CuEnv,
                               tx: Transaction,
                               changed: &[Pubkey],
                               error: Option<(u8, PercolatorError)>,
                               spl| {
                    transactions += 1;
                    rollbacks += usize::from(error.is_some());
                    peak = peak.max(land(env, tx, &tracked, changed, error, spl));
                };
                for step in 0..=6 {
                    if step == word_index % 7 {
                        for (from, to) in [(&admin, interim), (interim, &admin)] {
                            let ix = handoff(
                                &env,
                                asset,
                                processor::ASSET_AUTH_ADMIN,
                                from.pubkey(),
                                Some(to.pubkey()),
                                book.sequences[asset].authority_epoch,
                            );
                            let tx = signed(&env, &[ix], &[from, to]);
                            let economy = env.market_state();
                            run(&mut env, tx, &[market], None, 0);
                            book.profiles[asset].asset_admin = to.pubkey().to_bytes();
                            book.sequences[asset].authority_epoch += 1;
                            assert_eq!(env.market_state(), economy);
                            book.check(&env, &wallets, portfolio);
                        }
                    }
                    if step == 6 {
                        break;
                    }
                    env.svm.warp_to_slot(step as u64 + 2);
                    let role = word[step];
                    let from = holders[role];
                    let to = if from == 3 { originals[role] } else { 3 };
                    let ix = handoff(
                        &env,
                        asset,
                        kinds[role],
                        actors[from].pubkey(),
                        Some(actors[to].pubkey()),
                        book.sequences[asset].authority_epoch,
                    );
                    let tx = signed(&env, &[ix], &[actors[from], actors[to]]);
                    let economy = env.market_state();
                    run(&mut env, tx, &[market], None, 0);
                    holders[role] = to;
                    if role == 0 {
                        book.profiles[asset].oracle_authority = actors[to].pubkey().to_bytes();
                    } else {
                        set_holder(&mut book.profiles[asset], kinds[role], actors[to].pubkey());
                    }
                    book.sequences[asset].authority_epoch += 1;
                    assert_eq!(
                        env.market_state(),
                        economy,
                        "role management moves no quote or risk"
                    );
                    book.check(&env, &wallets, portfolio);

                    let funded_non_oracle = [
                        holders[1],
                        if insurance_policy { holders[2] } else { 0 },
                        if insurance_policy { 1 } else { holders[2] },
                    ]
                    .into_iter()
                    .find(|holder| *holder != holders[0])
                    .expect("at least one separately held funded role");
                    let wrong_scope = observe(
                        &env,
                        asset,
                        actors[funded_non_oracle].pubkey(),
                        book.sequences[asset].authority_epoch,
                        book.sequences[asset].oracle_observation + 1,
                    );
                    let mut signers = vec![actors[2]];
                    if funded_non_oracle != 2 {
                        signers.push(actors[funded_non_oracle]);
                    }
                    let tx = signed(&env, &[peer_payout.clone(), wrong_scope], &signers);
                    run(
                        &mut env,
                        tx,
                        &[],
                        Some((3, PercolatorError::Unauthorized)),
                        1,
                    );
                    book.check(&env, &wallets, portfolio);

                    let ix = observe(
                        &env,
                        asset,
                        actors[holders[0]].pubkey(),
                        book.sequences[asset].authority_epoch,
                        book.sequences[asset].oracle_observation + 1,
                    );
                    let tx = signed(&env, &[ix], &[actors[holders[0]]]);
                    run(&mut env, tx, &[market], None, 0);
                    book.profiles[asset].last_good_oracle_slot = step as u64 + 2;
                    book.sequences[asset].oracle_observation += 1;
                    book.check(&env, &wallets, portfolio);
                    for is_backing in [word_index % 2 == 0, word_index % 2 != 0] {
                        let recipient = if is_backing {
                            holders[1]
                        } else if insurance_policy {
                            1
                        } else {
                            holders[2]
                        };
                        let domain = asset * 2 + if is_backing { step % 2 } else { 0 };
                        let ix = payout(
                            &env,
                            actors[recipient].pubkey(),
                            wallets[recipient],
                            domain,
                            is_backing,
                            slices[step],
                        );
                        let tx = signed(&env, &[ix], &[actors[recipient]]);
                        run(&mut env, tx, &[market, vault, wallets[recipient]], None, 1);
                        book.pay(domain, is_backing, recipient, slices[step]);
                        book.check(&env, &wallets, portfolio);
                    }
                    assert_eq!(env.market_state().1.assets[peer], peer_economy);
                }
                assert_eq!(holders, originals);
                assert_eq!(book.sequences[asset].authority_epoch, initial_epoch + 14);
                // The initial signatures match the returned holders, but their
                // old asset epoch cannot authorize another funded succession.
                for tx in retained {
                    run(
                        &mut env,
                        tx,
                        &[],
                        Some((3, PercolatorError::EngineStale)),
                        1,
                    );
                    book.check(&env, &wallets, portfolio);
                }
                // Use a current sequence to isolate epoch checking from the
                // observation watermark after six accepted reports.
                let stale = observe(
                    &env,
                    asset,
                    actors[oracle_owner].pubkey(),
                    initial_epoch,
                    book.sequences[asset].oracle_observation + 1,
                );
                let mut signers = vec![actors[2]];
                if oracle_owner != 2 {
                    signers.push(actors[oracle_owner]);
                }
                let tx = signed(&env, &[peer_payout.clone(), stale], &signers);
                run(
                    &mut env,
                    tx,
                    &[],
                    Some((3, PercolatorError::EngineStale)),
                    1,
                );
                book.check(&env, &wallets, portfolio);
                let current = observe(
                    &env,
                    asset,
                    actors[oracle_owner].pubkey(),
                    book.sequences[asset].authority_epoch,
                    book.sequences[asset].oracle_observation + 1,
                );
                let tx = signed(&env, &[current], &[actors[oracle_owner]]);
                run(&mut env, tx, &[market], None, 0);
                book.sequences[asset].oracle_observation += 1;
                book.check(&env, &wallets, portfolio);
                for kind in &kinds[1..] {
                    let ix = handoff(
                        &env,
                        asset,
                        *kind,
                        admin.pubkey(),
                        Some(actors[3].pubkey()),
                        book.sequences[asset].authority_epoch,
                    );
                    let tx = signed(
                        &env,
                        &[peer_payout.clone(), ix],
                        &[actors[2], &admin, actors[3]],
                    );
                    run(
                        &mut env,
                        tx,
                        &[],
                        Some((3, PercolatorError::EngineLockActive)),
                        1,
                    );
                    book.check(&env, &wallets, portfolio);
                }
                run(
                    &mut env,
                    retained_peer,
                    &[market, vault, wallets[2]],
                    None,
                    1,
                );
                book.pay(peer * 2, true, 2, 1);
                book.check(&env, &wallets, portfolio);

                for domain in 0..4 {
                    let amount = book.backing[domain];
                    let ix = payout(&env, actors[2].pubkey(), wallets[2], domain, true, amount);
                    let tx = signed(&env, &[ix], &[actors[2]]);
                    run(&mut env, tx, &[market, vault, wallets[2]], None, 1);
                    book.pay(domain, true, 2, amount);
                    book.check(&env, &wallets, portfolio);
                }
                for target in [peer, asset] {
                    let amount = book.insurance[target * 2] + book.insurance[target * 2 + 1];
                    let ix = payout(
                        &env,
                        actors[1].pubkey(),
                        wallets[1],
                        target * 2,
                        false,
                        amount,
                    );
                    let tx = signed(&env, &[ix], &[actors[1]]);
                    run(&mut env, tx, &[market, vault, wallets[1]], None, 1);
                    book.pay(target * 2, false, 1, amount);
                    book.check(&env, &wallets, portfolio);
                }
                let exit = Instruction {
                    program_id: env.program_id,
                    data: env.withdraw_ix(portfolio, CAPITAL).encode(),
                    accounts: vec![
                        AccountMeta::new(actors[5].pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(wallets[5], false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                };
                let tx = signed(&env, &[exit], &[actors[5]]);
                run(
                    &mut env,
                    tx,
                    &[market, vault, portfolio, wallets[5]],
                    None,
                    1,
                );
                book.capital = 0;
                book.paid[5] = CAPITAL;
                book.check(&env, &wallets, portfolio);
                assert_eq!(
                    book.paid[4], 0,
                    "cold-admin round trips acquire no funded entitlement"
                );
                assert_eq!(
                    book.paid[0], 0,
                    "policy and oracle roles confer no live payout"
                );
                assert!(
                    book.paid[3] > 0,
                    "successor exercises its consented funded role"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 360);
    assert_eq!(rollbacks, 12 * worlds);
    eprintln!("INV-005 generated funded roles: {worlds} worlds, {transactions} checked transactions, {rollbacks} exact rollback/SPL prefixes, peak {peak} CU");
}
