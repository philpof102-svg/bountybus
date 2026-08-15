//! Value conservation and liveness under the **deployed** market parameters.
//!
//! Constants below are copied from `scripts/resources/config/market_configs.toml` (the three real
//! market sections agree; the fourth is an all-zero placeholder). Using the crate's fixture instead
//! would test the wrong protocol: its swap-impact factors are ~2.3x the deployed ones, its fee
//! split is 0.37 rather than 0.70, and its max_pool_value ceiling is ~8 orders of magnitude higher.
//!
//! Sizing: with max_pool_value_for_deposit = 1.75e27 and a token price of 2e13, deposits are
//! bounded to ~8.75e13 tokens. Amounts below stay inside that.
//!
//! Two questions:
//!   1. can a deposit -> withdraw round trip return more VALUE than it put in? (theft)
//!   2. can any accepted deposit wedge the market so an existing LP can no longer withdraw?
//!      (permanent freezing of funds)

use gmsol_model::{
    market::LiquidityMarketMutExt,
    params::{FeeParams, PriceImpactParams},
    price::Prices,
    test::{TestMarket, TestMarketConfig},
    LiquidityMarket, MarketAction,
};

const PRICE: u128 = 20_000_000_000_000; // 2e13, same convention as tests/borrowing_fee.rs
const MAX_POOL_VALUE: u128 = 1_750_000_000_000_000_000_000_000_000; // 1.75e27, market_configs.toml:123

fn deployed() -> TestMarket<u128, 20> {
    TestMarket::<u128, 20>::with_config(TestMarketConfig {
        // market_configs.toml lines 66-68
        swap_impact_params: PriceImpactParams::builder()
            .exponent(200_000_000_000_000_000_000) // 2.0
            .positive_factor(175_000_000_000)
            .negative_factor(350_000_000_000)
            .build(),
        // market_configs.toml lines 69-71
        swap_fee_params: FeeParams::builder()
            .fee_receiver_factor(70_000_000_000_000_000_000) // 0.70
            .positive_impact_fee_factor(50_000_000_000_000_000)
            .negative_impact_fee_factor(70_000_000_000_000_000)
            .build(),
        max_pool_value_for_deposit: MAX_POOL_VALUE,
        ..Default::default()
    })
}

#[test]
fn round_trip_conserves_value_on_deployed_params() -> gmsol_model::Result<()> {
    let p = Prices::new_for_test(PRICE, PRICE, PRICE);
    let mut market = deployed();
    // seed: 2e13 tokens each side -> 4e26 value each, under the 1.75e27 ceiling
    market.deposit(20_000_000_000_000, 20_000_000_000_000, p)?.execute()?;

    for amount in [1u128, 1_000_000, 1_000_000_000, 1_000_000_000_000] {
        let before = market.total_supply();
        market.deposit(amount, 0, p)?.execute()?;
        let minted = market.total_supply() - before;

        let report = market.withdraw(minted, p)?.execute()?;
        let out_value = (*report.long_token_output() + *report.short_token_output()) * PRICE;
        let in_value = amount * PRICE;
        let delta = out_value as i128 - in_value as i128;
        println!(
            "deposit={amount:>16} minted={minted:>22} delta_value={delta:>24} ({} bps)",
            if in_value > 0 { delta * 10_000 / in_value as i128 } else { 0 }
        );
        assert!(
            out_value <= in_value,
            "value extracted: in={in_value} out={out_value} minted={minted}"
        );
    }
    Ok(())
}

/// Liveness: after every deposit attempt (accepted or rejected), an existing LP must still be able
/// to withdraw. A state where withdrawals stop would be "permanent freezing of funds".
#[test]
fn no_deposit_wedges_existing_lp_withdrawals() -> gmsol_model::Result<()> {
    let p = Prices::new_for_test(PRICE, PRICE, PRICE);
    let mut market = deployed();
    market.deposit(20_000_000_000_000, 20_000_000_000_000, p)?.execute()?;
    let lp_tokens = market.total_supply();
    println!("honest LP holds {lp_tokens} market tokens");

    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut first_error: Option<String> = None;

    for exp in 0..24u32 {
        let amount = 10u128.saturating_pow(exp);
        let before = market.total_supply();
        match market.deposit(amount, 0, p).and_then(|d| d.execute()) {
            Ok(_) => {
                accepted += 1;
                let minted = market.total_supply() - before;
                if minted > 0 {
                    let _ = market.withdraw(minted, p)?.execute()?;
                }
            }
            Err(e) => {
                rejected += 1;
                if first_error.is_none() {
                    let msg = format!("{e:?}");
                    println!("first rejected deposit: 10^{exp} tokens -> {msg}");
                    first_error = Some(msg);
                }
            }
        }

        // THE LIVENESS CHECK
        let probe = market.total_supply() / 1_000;
        if probe > 0 {
            let res = market.withdraw(probe, p).and_then(|w| w.execute());
            assert!(
                res.is_ok(),
                "after a deposit attempt of 10^{exp} tokens the honest LP can NO LONGER withdraw: \
                 {:?} — permanent freezing of funds",
                res.err()
            );
            if let Ok(r) = res {
                let back = *r.long_token_output();
                if back > 0 {
                    let _ = market.deposit(back, 0, p).and_then(|d| d.execute());
                }
            }
        }
    }

    println!("deposit attempts: {accepted} accepted, {rejected} rejected; withdrawals stayed live throughout");
    Ok(())
}
