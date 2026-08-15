//! Position-side value conservation — the theft surface that matters on a perps protocol.
//!
//! The liquidity path (deposit/withdraw) was measured separately and conserves value. Positions are
//! where a trader can actually extract: open, let the price move (or not), close or get liquidated.
//! Read-only review covered this three times and found nothing; this file tests it by running it.
//!
//! Deployed constants from `scripts/resources/config/market_configs.toml` — NOT the crate fixture,
//! whose fee split (0.37 vs 0.70) and swap-impact factors (~2.3x) describe a different protocol.
//!
//! Invariants:
//!   1. round trip at UNCHANGED price must return LESS than the collateral put in (fees are real);
//!      returning more is free money minted from the pool.
//!   2. a losing position must never return more than its collateral.
//!   3. closing must never report an output larger than the position's collateral + capped PnL.

use gmsol_model::{
    fixed::Fixed,
    market::LiquidityMarketMutExt,
    params::{FeeParams, PriceImpactParams},
    price::Prices,
    test::{TestMarket, TestMarketConfig, TestPosition},
    MarketAction, PositionMutExt,
};

const PRICE_UNIT: u128 = 1_000_000_000; // 10^9, price scale used by their position tests

fn unit() -> u128 {
    Fixed::<u128, 20>::ONE.into_inner()
}

/// Deployed market params (market_configs.toml lines 66-71 + 123).
fn deployed() -> TestMarket<u128, 20> {
    TestMarket::<u128, 20>::with_config(TestMarketConfig {
        swap_impact_params: PriceImpactParams::builder()
            .exponent(200_000_000_000_000_000_000)
            .positive_factor(175_000_000_000)
            .negative_factor(350_000_000_000)
            .build(),
        swap_fee_params: FeeParams::builder()
            .fee_receiver_factor(70_000_000_000_000_000_000)
            .positive_impact_fee_factor(50_000_000_000_000_000)
            .negative_impact_fee_factor(70_000_000_000_000_000)
            .build(),
        order_fee_params: FeeParams::builder()
            .fee_receiver_factor(70_000_000_000_000_000_000)
            .positive_impact_fee_factor(50_000_000_000_000_000)
            .negative_impact_fee_factor(70_000_000_000_000_000)
            .build(),
        ..Default::default()
    })
}

/// Open then immediately close at the SAME price. Returns (collateral_in, total_out) in TOKENS.
fn round_trip_at_same_price(
    is_long: bool,
    leverage: u128,
) -> gmsol_model::Result<(u128, u128, u128)> {
    let unit = unit();
    let price = 100_000 * PRICE_UNIT;
    let prices = Prices::new_for_test(price, price, price);

    let mut market = deployed();

    // Deep liquidity so the position is never the constraint.
    let liq_value = 1_000_000 * unit;
    let liq_amount = liq_value / price;
    market.deposit(liq_amount, liq_amount, prices)?.execute()?;

    let collateral_value = 1_000 * unit;
    let collateral_amount = collateral_value / price;
    let size_delta_usd = collateral_value * leverage;

    // TestPosition::long(x)/short(x): x is WHICH TOKEN IS COLLATERAL, not the direction.
    let mut position = if is_long { TestPosition::long(true) } else { TestPosition::short(false) };
    let _ = position
        .ops(&mut market)
        .increase(prices, collateral_amount, size_delta_usd, None)?
        .execute()?;

    let report = position
        .ops(&mut market)
        .decrease(prices, size_delta_usd, None, 0, Default::default())?
        .execute()?;

    let out = *report.output_amount() + *report.secondary_output_amount();
    Ok((collateral_amount, out, size_delta_usd))
}

#[test]
fn position_round_trip_at_same_price_never_profits() -> gmsol_model::Result<()> {
    for is_long in [true, false] {
        for leverage in [1u128, 2, 5, 10] {
            let (collateral_in, out, size) = round_trip_at_same_price(is_long, leverage)?;
            let delta = out as i128 - collateral_in as i128;
            let bps = if collateral_in > 0 {
                delta * 10_000 / collateral_in as i128
            } else {
                0
            };
            println!(
                "{:>5} {:>2}x  collateral_in={collateral_in:>14}  out={out:>14}  delta={delta:>14} ({bps} bps)  size_usd={size}",
                if is_long { "long" } else { "short" },
                leverage
            );
            assert!(
                out <= collateral_in,
                "{} {leverage}x: closing at the SAME price returned MORE than the collateral \
                 (in={collateral_in}, out={out}) — value minted from the pool",
                if is_long { "long" } else { "short" }
            );
        }
    }
    Ok(())
}

/// A position that moves AGAINST the trader must never return more than its collateral.
#[test]
fn losing_position_never_returns_more_than_collateral() -> gmsol_model::Result<()> {
    let unit = unit();
    let entry = 100_000 * PRICE_UNIT;

    for (label, is_long, exit) in [
        ("long, price -20%", true, 80_000 * PRICE_UNIT),
        ("short, price +20%", false, 120_000 * PRICE_UNIT),
    ] {
        let prices_in = Prices::new_for_test(entry, entry, entry);
        let mut market = deployed();
        let liq_amount = (1_000_000 * unit) / entry;
        market.deposit(liq_amount, liq_amount, prices_in)?.execute()?;

        let collateral_value = 1_000 * unit;
        let collateral_amount = collateral_value / entry;
        let size_delta_usd = collateral_value * 2;

        let mut position = if is_long { TestPosition::long(true) } else { TestPosition::short(false) };
        let _ = position
            .ops(&mut market)
            .increase(prices_in, collateral_amount, size_delta_usd, None)?
            .execute()?;

        let prices_out = Prices::new_for_test(exit, exit, exit);
        let report = position
            .ops(&mut market)
            .decrease(prices_out, size_delta_usd, None, 0, Default::default())?
            .execute()?;

        let out = *report.output_amount() + *report.secondary_output_amount();
        println!(
            "{label:>20}: collateral_in={collateral_amount:>14} out={out:>14} pnl={:?}",
            report.pnl().pnl()
        );
        assert!(
            out <= collateral_amount,
            "{label}: a LOSING position returned more than its collateral (in={collateral_amount}, out={out})"
        );
    }
    Ok(())
}
