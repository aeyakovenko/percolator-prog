//! INV-005/024/027/055: rejected zeroing restores a funded handoff and its SPL payout.
//! Retained consent then commits; cold-admin management cannot acquire the successor's stock.

use super::{handoff, land, profile, set_holder, signed};
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::*;

const BACKING: [u128; 4] = [31, 43, 59, 71];
const INSURANCE: [u128; 4] = [17, 23, 29, 37];
const CAPITAL: u128 = 101;
const SUPPLY: u128 = 411;
const ROLES: [u8; 3] = [
    processor::ASSET_AUTH_INSURANCE,
    processor::ASSET_AUTH_INSURANCE_OPERATOR,
    processor::ASSET_AUTH_BACKING_BUCKET,
];

struct Book {
    backing: [u128; 4],
    insurance: [u128; 4],
    wallets: [u128; 6],
}

impl Book {
    fn pay(&mut self, asset: usize, backing: bool, recipient: usize, amount: u128) {
        let domain = asset * 2;
        if backing {
            self.backing[domain] -= amount;
        } else {
            let long = amount.min(self.insurance[domain]);
            self.insurance[domain] -= long;
            self.insurance[domain + 1] -= amount - long;
        }
        self.wallets[recipient] += amount;
    }

    fn check(&self, env: &V16CuEnv, wallets: &[Pubkey; 6]) {
        let (_, group) = env.market_state();
        let insurance = self.insurance.iter().sum::<u128>();
        let vault = CAPITAL + insurance + self.backing.iter().sum::<u128>();
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.c_tot, CAPITAL);
        assert_eq!(group.insurance, insurance);
        assert_eq!(group.vault, vault);
        assert_eq!(env.token_amount(env.vault) as u128, vault);
        assert_eq!(&group.insurance_domain_budget[..4], &self.insurance);
        for (bucket, amount) in group.source_backing_buckets.iter().zip(self.backing) {
            assert_eq!(bucket.fresh_unliened_backing_num, amount * BOUND_SCALE);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(bucket.utilization_fee_earnings, 0);
        }
        for (wallet, amount) in wallets.iter().zip(self.wallets) {
            let token = TokenAccount::unpack(&env.svm.get_account(wallet).unwrap().data).unwrap();
            assert_eq!(token.mint, env.mint);
            assert_eq!(token.amount as u128, amount);
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply as u128, SUPPLY);
        assert_eq!(vault + self.wallets.iter().sum::<u128>(), SUPPLY);
    }
}

fn payout(
    env: &V16CuEnv,
    holder: Pubkey,
    wallet: Pubkey,
    asset: usize,
    backing: bool,
    amount: u128,
    epoch: u64,
) -> Instruction {
    let market_id = env.asset_market_id(asset as u16);
    let ix = if backing {
        ProgInstruction::WithdrawBackingBucket {
            domain: (asset * 2) as u16,
            market_id,
            authority_epoch: epoch,
            amount,
        }
    } else {
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id,
            authority_epoch: epoch,
            amount,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(holder, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ix.encode(),
    }
}

fn fund_fixture(env: &mut V16CuEnv, actors: &[&Keypair; 6]) -> ([Pubkey; 6], Pubkey) {
    for actor in &actors[..4] {
        env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
    }
    env.svm.airdrop(&actors[5].pubkey(), 1_000_000_000).unwrap();
    let wallets =
        actors.map(|actor| create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint));
    for (actor, amount) in [
        (0, INSURANCE.iter().sum()),
        (2, BACKING.iter().sum()),
        (5, CAPITAL),
    ] {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &wallets[actor],
                &actors[4].pubkey(),
                &[],
                amount as u64,
            )
            .unwrap(),
            &[actors[4]],
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
            &actors[4].pubkey(),
            &[],
        )
        .unwrap(),
        &[actors[4]],
    )
    .unwrap();
    for asset in 0..2 {
        for (kind, actor) in ROLES.into_iter().zip(actors) {
            env.try_update_per_asset_authority_with_cu(
                actors[4],
                Some(actor),
                asset,
                kind,
                actor.pubkey().to_bytes(),
            )
            .unwrap();
        }
    }
    let user = actors[5];
    let portfolio_key = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio_key,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = portfolio_key.pubkey();
    let mut accounts = vec![
        AccountMeta::new(user.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    env.send(ProgInstruction::InitPortfolio, accounts.clone(), &[user])
        .unwrap();
    accounts.extend([
        AccountMeta::new(wallets[5], false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]);
    env.send(env.deposit_ix(portfolio, CAPITAL), accounts, &[user])
        .unwrap();
    for domain in 0..4 {
        for backing in [false, true] {
            let asset = domain / 2;
            let seq = env.control_sequences(asset);
            let market_id = env.asset_market_id(asset as u16);
            let (actor, ix) = if backing {
                (
                    2,
                    ProgInstruction::TopUpBackingBucket {
                        domain: domain as u16,
                        market_id,
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.backing_top_up),
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: BACKING[domain],
                        expiry_slot: 10_000,
                    },
                )
            } else {
                (
                    0,
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: domain as u16,
                        market_id,
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.insurance_top_up),
                        amount: INSURANCE[domain],
                    },
                )
            };
            env.send(
                ix,
                vec![
                    AccountMeta::new(actors[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(wallets[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[actors[actor]],
            )
            .unwrap();
        }
    }
    (wallets, portfolio)
}

#[test]
fn v16_program_zero_role_suffix_restores_funded_handoff_payout_and_retained_consent() {
    assert_eq!(
        SUPPLY,
        CAPITAL + BACKING.iter().sum::<u128>() + INSURANCE.iter().sum::<u128>()
    );
    let mut peak = [0; 3]; // zero rollback, cold-admin rollback, committed payout
    let mut worlds = 0;
    for role in [1, 2] {
        for asset in 0..2 {
            for zero_by_cold in [false, true] {
                let peer = 1 - asset;
                let backing = role == 2;
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let holders = [Keypair::new(), Keypair::new(), Keypair::new()];
                let successor = Keypair::new();
                let cold = env.admin.insecure_clone();
                let user = Keypair::new();
                let actors = [
                    &holders[0],
                    &holders[1],
                    &holders[2],
                    &successor,
                    &cold,
                    &user,
                ];
                let (wallets, portfolio) = fund_fixture(&mut env, &actors);
                let user_before = env.svm.get_account(&portfolio);
                let initial = env.market_state();
                let mut profiles = [profile(&env, 0), profile(&env, 1)];
                let mut sequences = [env.control_sequences(0), env.control_sequences(1)];
                let mut book = Book {
                    backing: BACKING,
                    insurance: INSURANCE,
                    wallets: [0; 6],
                };
                let market = env.market;
                let vault = env.vault;
                let mut tracked = vec![market, vault, env.mint, env.vault_authority, portfolio];
                tracked.extend(wallets);
                tracked.extend(actors.map(Signer::pubkey));
                let check = |env: &V16CuEnv, book: &Book, profiles: &[_; 2], sequences: &[_; 2]| {
                    book.check(env, &wallets);
                    assert_eq!([profile(env, 0), profile(env, 1)], *profiles);
                    assert_eq!(
                        [env.control_sequences(0), env.control_sequences(1)],
                        *sequences
                    );
                    assert_eq!(env.market_state().0, initial.0);
                    assert_eq!(env.market_state().1.assets, initial.1.assets);
                    assert_eq!(env.svm.get_account(&portfolio), user_before);
                    for (wallet, actor) in wallets.iter().zip(actors) {
                        let token =
                            TokenAccount::unpack(&env.svm.get_account(wallet).unwrap().data)
                                .unwrap();
                        assert_eq!(token.owner, actor.pubkey());
                    }
                };
                check(&env, &book, &profiles, &sequences);
                let epoch = sequences[asset].authority_epoch;
                env.svm.expire_blockhash();
                let old_payout = payout(
                    &env,
                    actors[role].pubkey(),
                    wallets[role],
                    asset,
                    backing,
                    3,
                    epoch,
                );
                let retained_old = signed(&env, &[old_payout], &[actors[role]]);
                let transfer = handoff(
                    &env,
                    asset,
                    ROLES[role],
                    actors[role].pubkey(),
                    Some(successor.pubkey()),
                    epoch,
                );
                let partial = payout(
                    &env,
                    successor.pubkey(),
                    wallets[3],
                    asset,
                    backing,
                    5,
                    epoch + 1,
                );
                let retained_pair = signed(
                    &env,
                    &[transfer.clone(), partial.clone()],
                    &[actors[role], &successor],
                );
                for tx in [&retained_old, &retained_pair] {
                    let mut keys = tracked.clone();
                    keys.extend_from_slice(&tx.message.account_keys);
                    let before = super::frame(&env, &keys);
                    env.svm
                        .simulate_transaction(tx.clone().into())
                        .expect("retained funded consent is initially live");
                    assert_eq!(super::frame(&env, &keys), before);
                }

                // The suffix observes the successor role and its already-paid partial stock.
                // Its rejection must restore both the handoff epoch and the real SPL transfer.
                let zero_signer = if zero_by_cold { &cold } else { &successor };
                let zero = handoff(
                    &env,
                    asset,
                    ROLES[role],
                    zero_signer.pubkey(),
                    None,
                    epoch + 1,
                );
                let mut signers = vec![actors[role], &successor];
                if zero_by_cold {
                    signers.push(&cold);
                }
                let tx = signed(&env, &[transfer, partial, zero], &signers);
                peak[0] = peak[0].max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((4, PercolatorError::InvalidInstruction)),
                    1,
                ));
                check(&env, &book, &profiles, &sequences);
                assert_eq!(env.market_state(), initial);

                peak[2] = peak[2].max(land(
                    &mut env,
                    retained_old,
                    &tracked,
                    &[market, vault, wallets[role]],
                    None,
                    1,
                ));
                book.pay(asset, backing, role, 3);
                check(&env, &book, &profiles, &sequences);
                peak[2] = peak[2].max(land(
                    &mut env,
                    retained_pair,
                    &tracked,
                    &[market, vault, wallets[3]],
                    None,
                    1,
                ));
                book.pay(asset, backing, 3, 5);
                set_holder(&mut profiles[asset], ROLES[role], successor.pubkey());
                sequences[asset].authority_epoch += 1;
                check(&env, &book, &profiles, &sequences);

                // After the committed handoff, an unchanged sibling payout executes before
                // either form of cold-admin management rejects; that payout remains retryable.
                let peer_payout = payout(
                    &env,
                    actors[role].pubkey(),
                    wallets[role],
                    peer,
                    backing,
                    7,
                    sequences[peer].authority_epoch,
                );
                let retained_peer = signed(&env, &[peer_payout.clone()], &[actors[role]]);
                for (destination, error) in [
                    (None, PercolatorError::InvalidInstruction),
                    (Some(cold.pubkey()), PercolatorError::EngineLockActive),
                ] {
                    let management = handoff(
                        &env,
                        asset,
                        ROLES[role],
                        cold.pubkey(),
                        destination,
                        epoch + 1,
                    );
                    let tx = signed(
                        &env,
                        &[peer_payout.clone(), management],
                        &[actors[role], &cold],
                    );
                    peak[1] = peak[1].max(land(&mut env, tx, &tracked, &[], Some((3, error)), 1));
                    check(&env, &book, &profiles, &sequences);
                }
                let old = payout(
                    &env,
                    actors[role].pubkey(),
                    wallets[role],
                    asset,
                    backing,
                    1,
                    epoch + 1,
                );
                let tx = signed(&env, &[old], &[actors[role]]);
                peak[1] = peak[1].max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((2, PercolatorError::Unauthorized)),
                    0,
                ));
                check(&env, &book, &profiles, &sequences);

                peak[2] = peak[2].max(land(
                    &mut env,
                    retained_peer,
                    &tracked,
                    &[market, vault, wallets[role]],
                    None,
                    1,
                ));
                book.pay(peer, backing, role, 7);
                check(&env, &book, &profiles, &sequences);
                let remaining = if backing {
                    book.backing[asset * 2]
                } else {
                    book.insurance[asset * 2] + book.insurance[asset * 2 + 1]
                };
                let final_payout = payout(
                    &env,
                    successor.pubkey(),
                    wallets[3],
                    asset,
                    backing,
                    remaining,
                    epoch + 1,
                );
                let tx = signed(&env, &[final_payout], &[&successor]);
                peak[2] = peak[2].max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[market, vault, wallets[3]],
                    None,
                    1,
                ));
                book.pay(asset, backing, 3, remaining);
                check(&env, &book, &profiles, &sequences);
                let original = if backing {
                    BACKING[asset * 2]
                } else {
                    INSURANCE[asset * 2] + INSURANCE[asset * 2 + 1]
                };
                assert_eq!(book.wallets[3], original - 3);
                assert_eq!(book.wallets[role], 3 + 7);
                assert_eq!(
                    book.wallets[4], 0,
                    "cold admin receives no attributed stock"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    eprintln!("INV-005 funded zero transition: worlds={worlds}, simulations=16, exact_rejections=32, rolled_back_handoffs=8, rolled_back_SPL_payouts=24, committed_handoffs=8, committed_payouts=32, peak CU [zero rollback, cold/old-role rejection, payout]={peak:?}");
}
