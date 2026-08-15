//! Time-dependent fees: borrowing accrual — the surface every earlier harness missed.
//!
//! The liquidity, position and liquidation harnesses all opened and closed inside the same instant,
//! so `move_clock_forward` was never called and borrowing fees never accrued. A protocol can look
//! perfectly conservative under instant round trips and still let a trader hold leverage for free.
//!
//! Deployed constants from `scripts/resources/config/market_configs.toml`.
//!
//! Note on scenario design: the borrowing rate comes from a KINK model driven by pool *usage*. A
//! $5,000 position against a $10,000,000 pool is 0.05% usage, where a near-zero rate is expected
//! and correct. So the pool size is a parameter here, and the interesting test sweeps it.

use std::time::Duration;

use gmsol_model::{
    fixed::Fixed,
    market::LiquidityMarketMutExt,
    params::{FeeParams, PriceImpactParams},
    price::Prices,
    test::{TestMarket, TestMarketConfig, TestPosition},
    BorrowingFeeMarketExt, BorrowingFeeMarketMutExt, MarketAction, PositionMutExt,
};

const PRICE_UNIT: u128 = 1_000_000_000;
const DAY: u64 = 3600 * 24;

fn unit() -> u128 {
    Fixed::<u128, 20>::ONE.into_inner()
}

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

/// Open a 5x long against a pool of `pool_units`, hold, close at the SAME price.
/// Price never moves, so every difference in output is fees. Returns (collateral, out, rate).
fn hold_and_close(hold_secs: u64, pool_units: u128) -> gmsol_model::Result<(u128, u128, u128)> {
    hold_and_close_inner(hold_secs, pool_units, false)
}

/// `keeper_update` models what `programs/store` does in `update_fees_state()` — logged there as
/// "[Pre-execute] borrowing state updated" — refreshing the borrowing state BEFORE every order
/// executes. A model-level test that omits it charges no borrowing fee no matter how long you wait,
/// which is a property of the test, not of the protocol.
fn hold_and_close_inner(
    hold_secs: u64,
    pool_units: u128,
    keeper_update: bool,
) -> gmsol_model::Result<(u128, u128, u128)> {
    let unit = unit();
    let price = 100_000 * PRICE_UNIT;
    let prices = Prices::new_for_test(price, price, price);
    let mut market = deployed();

    let liq_amount = (pool_units * unit) / price;
    market.deposit(liq_amount, liq_amount, prices)?.execute()?;

    let collateral_value = 1_000 * unit;
    let collateral_amount = collateral_value / price;
    let size_delta_usd = collateral_value * 5;

    // Pre-execute for the OPENING order. This matters more than it looks: TestMarket initialises
    // each clock lazily (`clocks.entry(kind).or_insert(now)`), so the FIRST call always reports 0
    // seconds elapsed. Without an update here, a single update at close time sees zero elapsed no
    // matter how long the position was held.
    if keeper_update {
        let _ = market.update_borrowing(&prices)?.execute()?;
    }

    let mut position = TestPosition::long(true);
    let _ = position
        .ops(&mut market)
        .increase(prices, collateral_amount, size_delta_usd, None)?
        .execute()?;

    let rate = market.borrowing_factor_per_second(true, &prices)?;

    if hold_secs > 0 {
        market.move_clock_forward(Duration::from_secs(hold_secs));
    }

    if keeper_update {
        let _ = market.update_borrowing(&prices)?.execute()?;
    }

    let report = position
        .ops(&mut market)
        .decrease(prices, size_delta_usd, None, 0, Default::default())?
        .execute()?;

    let out = *report.output_amount() + *report.secondary_output_amount();
    Ok((collateral_amount, out, rate))
}

#[test]
fn accrual_is_monotonic_in_time() -> gmsol_model::Result<()> {
    let mut prev: Option<(u64, u128)> = None;
    for days in [0u64, 1, 7, 30, 365] {
        let (collateral, out, rate) = hold_and_close(days * DAY, 100_000)?;
        let cost = collateral as i128 - out as i128;
        println!("hold {days:>4}d: out={out} cost={cost} rate_per_sec={rate}");
        if let Some((prev_days, prev_out)) = prev {
            assert!(
                out <= prev_out,
                "holding {days}d returned MORE than holding {prev_days}d ({out} > {prev_out}) — \
                 waiting longer became cheaper; accrual is not monotonic"
            );
        }
        prev = Some((days, out));
    }
    Ok(())
}

/// Sweep pool size, i.e. usage. Reports whether holding for a year costs more than closing
/// instantly, and the borrowing rate that drives it. A zero rate at negligible usage is expected;
/// a zero rate at HIGH usage would mean leverage is free.
#[test]
fn borrowing_accrues_once_usage_is_material() -> gmsol_model::Result<()> {
    let mut any_accrual = false;
    for pool in [10_000_000u128, 1_000_000, 100_000, 20_000, 10_000] {
        let (_, out_instant, rate) = hold_and_close(0, pool)?;
        let (_, out_year, _) = hold_and_close(365 * DAY, pool)?;
        let extra = out_instant as i128 - out_year as i128;
        // position is 5,000 USD of size; usage ~ size / pool
        println!(
            "pool={pool:>10}u (usage~{:.2}%): rate_per_sec={rate:>22}  extra_cost_of_1y_hold={extra}",
            5_000.0 * 100.0 / pool as f64
        );
        if extra > 0 {
            any_accrual = true;
        }
    }
    assert!(
        any_accrual,
        "borrowing fees never accrued at ANY usage level — holding leverage for a year is free"
    );
    Ok(())
}

/// Churning short holds must not be cheaper than one continuous hold, or a trader keeps exposure
/// while dodging borrowing fees.
#[test]
fn churning_is_not_cheaper_than_holding() -> gmsol_model::Result<()> {
    let unit = unit();
    let price = 100_000 * PRICE_UNIT;
    let prices = Prices::new_for_test(price, price, price);

    let (collateral, out_hold, _) = hold_and_close(360 * DAY, 100_000)?;
    let cost_hold = collateral as i128 - out_hold as i128;

    let mut market = deployed();
    let liq_amount = (100_000 * unit) / price;
    market.deposit(liq_amount, liq_amount, prices)?.execute()?;

    let collateral_value = 1_000 * unit;
    let collateral_amount = collateral_value / price;
    let size_delta_usd = collateral_value * 5;

    let mut cost_churn: i128 = 0;
    for _ in 0..12 {
        let mut position = TestPosition::long(true);
        let _ = position
            .ops(&mut market)
            .increase(prices, collateral_amount, size_delta_usd, None)?
            .execute()?;
        market.move_clock_forward(Duration::from_secs(30 * DAY));
        let report = position
            .ops(&mut market)
            .decrease(prices, size_delta_usd, None, 0, Default::default())?
            .execute()?;
        let out = *report.output_amount() + *report.secondary_output_amount();
        cost_churn += collateral_amount as i128 - out as i128;
    }

    println!("continuous 360d cost={cost_hold}   12x30d churn cost={cost_churn}");
    assert!(
        cost_churn >= cost_hold,
        "churning 12x30d cost LESS than holding 360d ({cost_churn} < {cost_hold}) — \
         a trader keeps exposure while dodging borrowing fees"
    );
    Ok(())
}

/// The same sweep, but running the keeper's pre-execute borrowing update first.
#[test]
fn borrowing_accrues_when_the_keeper_update_runs() -> gmsol_model::Result<()> {
    for pool in [1_000_000u128, 100_000, 20_000] {
        let (_, out_instant, rate) = hold_and_close_inner(0, pool, true)?;
        let (_, out_year, _) = hold_and_close_inner(365 * DAY, pool, true)?;
        let extra = out_instant as i128 - out_year as i128;
        println!(
            "WITH keeper update  pool={pool:>9}u: rate_per_sec={rate:>22} extra_cost_of_1y_hold={extra}"
        );
        assert!(
            extra > 0,
            "even WITH the keeper's borrowing update, a one-year hold cost nothing extra at pool={pool}"
        );
    }
    Ok(())
}
