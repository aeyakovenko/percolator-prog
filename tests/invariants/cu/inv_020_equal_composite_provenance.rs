//! INV-020/045/053/054: equal composite prices do not make component histories interchangeable.
//! Existing composite matrices vary timestamps with fixed component prices; the single-feed
//! same-epoch test changes the final price. Neither distinguishes component provenance from
//! aggregate-price equality while an exposed Hybrid mark retains fractional movement credit.
//! System/SPL/wrapper instructions construct the economic state. Only provider reports and
//! Clock are external fixtures. This is bounded conformance evidence, not status promotion.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::support::fuzz_model::assert_current_certificate_matches_independent;

const ENTRY: u64 = 100;
const TARGET: u64 = 120;
const CAPITAL: u128 = 1_000;
const CAP_BPS: u64 = 37;
const FLAGS: u8 = ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3;
const SCALE: u32 = 10_000;
const COMPONENTS: [[u64; 3]; 2] = [
    [7_200_000, 2_000_000, 3_000_000],
    [14_400_000, 4_000_000, 3_000_000],
];

fn observe(
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    asset: u16,
    reports: &[Pubkey; 3],
    caller_slot: u64,
) -> Result<u64, String> {
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    accounts.extend(reports.map(|key| AccountMeta::new_readonly(key, false)));
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: caller_slot,
            observations: crank_observations_with_accounts(asset, 3),
        },
        accounts,
        &[],
    )
}

fn profile(env: &V16CuEnv, asset: u16) -> state::AssetOracleProfileV16 {
    state::read_asset_oracle_profile(
        &env.svm.get_account(&env.market).unwrap().data,
        asset as usize,
    )
    .unwrap()
}

fn assert_movement(env: &V16CuEnv, asset: u16, slot: u64) {
    // Whole-interval arithmetic does not call the production clamp or recurrence. Each
    // slot earns 0.37 price atoms: two sub-atom steps MUST combine into an atom on step 3.
    let numerator = ENTRY * CAP_BPS * (slot - 1);
    let effective = ENTRY + numerator / 10_000;
    let group = env.market_state().1;
    let market = group.assets[asset as usize];
    assert_eq!(group.current_slot, slot);
    assert_eq!(market.slot_last, slot);
    assert_eq!(market.raw_oracle_target_price, TARGET);
    assert_eq!(market.effective_price, effective);
    assert_eq!(market.fund_px_last, ENTRY);
    assert_eq!(
        market.k_long,
        i128::from(effective - ENTRY) * ADL_ONE as i128
    );
    assert_eq!(market.k_short, -market.k_long);
    assert_eq!((market.f_long_num, market.f_short_num), (0, 0));
    assert_eq!(
        (market.oi_eff_long_q, market.oi_eff_short_q),
        (POS_SCALE, POS_SCALE)
    );
    let oracle = profile(env, asset);
    assert_eq!(oracle.oracle_target_price_e6, TARGET);
    assert_eq!(
        u64::from(oracle.price_move_remainder_bps_num),
        numerator % 10_000
    );
}

#[test]
fn v16_program_equal_composite_refresh_preserves_component_provenance_and_mark_carry() {
    let providers = [
        EpochMatrixProvider::Pyth,
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ];
    for prices in COMPONENTS {
        assert_eq!(
            reference_composite_price_e6(&prices, FLAGS, 0, SCALE),
            TARGET
        );
    }
    let mut peak_cu = 0;
    let mut rejected = 0;
    let mut current_noops = 0;
    let mut certified = 0;
    for asset in [0, 1] {
        for rotation in 0..3 {
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    initial_price: ENTRY,
                    max_price_move_bps_per_slot: CAP_BPS,
                    ..V16CuMarketParams::default()
                },
            );
            set_test_clock(&mut env, 1, 100);
            let initial = [6_000_000, 2_000_000, 3_000_000];
            let legs = std::array::from_fn::<_, 3, _>(|index| {
                new_epoch_matrix_leg(
                    &mut env,
                    providers[(index + rotation) % 3],
                    rotation,
                    index,
                    initial[index],
                    100,
                    1,
                )
            });
            let reports = legs.map(|leg| leg.account);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                asset,
                3,
                FLAGS,
                legs.map(|leg| leg.feed),
                &reports,
                1,
                100,
                0,
                SCALE,
                100,
                100,
            )
            .expect("configure the initial composite at 100");
            let owners = std::array::from_fn::<_, 3, _>(|_| Keypair::new());
            let portfolios = owners.each_ref().map(|owner| {
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
                    &[owner],
                )
                .unwrap();
                env.portfolios.push(portfolio.pubkey());
                portfolio.pubkey()
            });
            let tokens = [0, 1].map(|actor| {
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
                        CAPITAL as u64,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
                env.send(
                    env.deposit_ix(portfolios[actor], CAPITAL),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .unwrap();
                token
            });
            let [short, long, keeper] = portfolios;
            env.trade_asset_with_cu(
                asset,
                &owners[0],
                short,
                &owners[1],
                long,
                -(POS_SCALE as i128),
                ENTRY,
                0,
            );
            let initial_cert = health_cert(&env.portfolio_state(short));
            assert!(initial_cert.valid);
            assert_eq!(initial_cert.certified_initial_req, u128::from(ENTRY));
            let untouched = [short, long].map(|key| env.svm.get_account(&key).unwrap());
            let mut keys = vec![env.market, env.mint, env.vault, env.admin.pubkey()];
            keys.extend(portfolios);
            keys.extend(tokens);
            keys.extend(reports);
            keys.extend(owners.each_ref().map(Signer::pubkey));
            // Include full economic/provider Account frames; only the network fee payer is excluded.
            let frame = |env: &V16CuEnv| {
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let write = |env: &mut V16CuEnv, prices: [u64; 3], time, slot| {
                for index in 0..3 {
                    write_epoch_matrix_leg(env, legs[index], prices[index], time, slot);
                }
            };

            set_test_clock(&mut env, 2, 101);
            write(&mut env, COMPONENTS[0], 101, 2);
            peak_cu = peak_cu.max(observe(&mut env, keeper, asset, &reports, u64::MAX).unwrap());
            assert_movement(&env, asset, 2);
            assert_eq!(
                [short, long].map(|key| env.svm.get_account(&key).unwrap()),
                untouched
            );
            assert!(initial_cert.cert_oracle_epoch < env.market_state().1.oracle_epoch);

            let mut accepted = 0;
            let mut publish_time = 101;
            for slot in 2..=4 {
                let now = 100 + 2 * (slot - 1) as i64;
                set_test_clock(&mut env, slot, now);
                if slot > 2 {
                    let previous_good_slot = profile(&env, asset).last_good_oracle_slot;
                    peak_cu = peak_cu.max(observe(&mut env, keeper, asset, &reports, 0).unwrap());
                    assert_eq!(
                        profile(&env, asset).last_good_oracle_slot,
                        previous_good_slot
                    );
                    assert_movement(&env, asset, slot);
                }
                let replacement = 1 - accepted;
                // Equal composite output cannot excuse changing any committed component at
                // its old epoch. The second rejection also proves a same-price fresh report
                // actually replaced stored components instead of only advancing a timestamp.
                for after_fresh_acceptance in [false, true] {
                    let prices = if after_fresh_acceptance {
                        COMPONENTS[accepted]
                    } else {
                        COMPONENTS[replacement]
                    };
                    let time = if after_fresh_acceptance {
                        now
                    } else {
                        publish_time
                    };
                    write(&mut env, prices, time, slot);
                    let before = frame(&env);
                    let error = observe(&mut env, short, asset, &reports, u64::MAX)
                        .expect_err("same-epoch component replacement must reject even at equal composite price");
                    assert!(
                        error.contains("Custom(26)"),
                        "expected OracleInvalid: {error}"
                    );
                    assert_eq!(
                        frame(&env),
                        before,
                        "component rejection must roll back every account"
                    );
                    rejected += 1;
                    if !after_fresh_acceptance {
                        write(&mut env, COMPONENTS[replacement], now, slot);
                        let asset_before = env.market_state().1.assets[asset as usize];
                        peak_cu =
                            peak_cu.max(observe(&mut env, keeper, asset, &reports, 0).unwrap());
                        assert_eq!(env.market_state().1.assets[asset as usize], asset_before);
                        let oracle = profile(&env, asset);
                        assert_eq!(oracle.oracle_leg_prices_e6, COMPONENTS[replacement]);
                        assert_eq!(oracle.oracle_leg_publish_times, [now; 3]);
                        assert_eq!(oracle.oracle_target_publish_time, now);
                        assert_eq!(oracle.last_good_oracle_slot, slot);
                        assert_movement(&env, asset, slot);
                    }
                }
                accepted = replacement;
                publish_time = now;
                write(&mut env, COMPONENTS[accepted], publish_time, slot);
                let before = frame(&env);
                let refresh = observe(&mut env, short, asset, &reports, u64::MAX);
                if slot == 3 {
                    // The second sub-atom step changes neither K/F nor target. Fresh provider
                    // provenance alone does not require recertifying already exact health.
                    let error = refresh.expect_err("unchanged current health has no account work");
                    assert!(
                        error.contains("Custom(22)"),
                        "expected EngineNonProgress: {error}"
                    );
                    assert_eq!(frame(&env), before);
                    current_noops += 1;
                } else {
                    peak_cu = peak_cu
                        .max(refresh.expect("target change or whole-atom loss refreshes health"));
                }
                assert_movement(&env, asset, slot);
                let account = env.portfolio_state(short);
                let cert = health_cert(&account);
                let loss = u128::from(ENTRY * CAP_BPS * (slot - 1) / 10_000);
                assert_eq!(account.capital.get(), CAPITAL - loss);
                assert_eq!(cert.certified_equity, (CAPITAL - loss) as i128);
                // With unit short exposure and 100% margins, effective notional plus
                // adverse target lag remains exactly 120 even after the first paid loss.
                assert_eq!(cert.certified_initial_req, u128::from(TARGET));
                assert_eq!(cert.certified_maintenance_req, u128::from(TARGET));
                assert!(assert_current_certificate_matches_independent(
                    "equal composite provenance",
                    &env.market_state().1,
                    &account,
                )
                .unwrap());
                assert_eq!(env.svm.get_account(&long).unwrap(), untouched[1]);
                assert_eq!(env.token_amount(env.vault), 2 * CAPITAL as u64);
                assert_eq!(tokens.map(|key| env.token_amount(key)), [0, 0]);
                certified += 1;
            }
        }
    }
    assert_eq!((rejected, certified), (36, 18));
    assert_eq!(current_noops, 6);
    assert_cu_within(
        "equal composite provenance and carry",
        peak_cu,
        CRANK_CU_LIMIT,
    );
    println!("equal composite conformance: 6 worlds, {rejected} provenance rollbacks, {current_noops} exact current-health no-ops, {certified} lag certificates, peak {peak_cu} CU");
}
