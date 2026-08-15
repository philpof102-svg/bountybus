//! Liquidation, insolvent close, and PnL capping (ADL) — the last untested value surface.
//!
//! Earlier harnesses covered the liquidity path and ordinary position open/close, both passing
//! `DecreasePositionFlags::default()` (every flag false), which means the LIQUIDATION path was
//! never exercised at all. This file drives it explicitly.
//!
//! Deployed constants from `scripts/resources/config/market_configs.toml`.
//!
//! Invariants:
//!   1. Liquidating an underwater position must never pay the trader more than their collateral.
//!   2. An INSOLVENT close (loss exceeds collateral) must pay the trader nothing and flag the
//!      shortfall — the pool absorbs it. Paying out here would mint value from other LPs.
//!   3. Being liquidated must never pay MORE than closing normally at the same price: otherwise a
//!      trader is paid a bonus for getting liquidated.
//!   4. Capped PnL must actually cap: a hugely profitable close must report pnl < uncapped_pnl.

use gmsol_model::{
    action::decrease_position::DecreasePositionFlags,
    fixed::Fixed,
    market::LiquidityMarketMutExt,
    params::{FeeParams, PriceImpactParams},
    price::Prices,
    test::{MaxPnlFactors, TestMarket, TestMarketConfig, TestPosition},
    MarketAction, PositionMutExt,
};

const PRICE_UNIT: u128 = 1_000_000_000;

fn unit() -> u128 {
    Fixed::<u128, 20>::ONE.into_inner()
}

fn deployed_with(max_pnl: Option<MaxPnlFactors<u128>>) -> TestMarket<u128, 20> {
    let mut cfg = TestMarketConfig::<u128, 20> {
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
    };
    if let Some(f) = max_pnl {
        cfg.max_pnl_factors = f;
    }
    TestMarket::<u128, 20>::with_config(cfg)
}

fn liquidation_flags() -> DecreasePositionFlags {
    DecreasePositionFlags {
        is_insolvent_close_allowed: true,
        is_liquidation_order: true,
        is_cap_size_delta_usd_allowed: true,
    }
}

/// Open a leveraged long, move the price, then close with the given flags.
/// Returns (collateral_amount, output_amount, pnl, uncapped_pnl, insolvent_step).
#[allow(clippy::type_complexity)]
fn open_move_close(
    leverage: u128,
    exit_price: u128,
    flags: DecreasePositionFlags,
    max_pnl: Option<MaxPnlFactors<u128>>,
    pool_units: u128,
) -> gmsol_model::Result<(u128, u128, i128, i128, String)> {
    let unit = unit();
    let entry = 100_000 * PRICE_UNIT;
    let prices_in = Prices::new_for_test(entry, entry, entry);
    let mut market = deployed_with(max_pnl);

    let liq_amount = (pool_units * unit) / entry;
    market.deposit(liq_amount, liq_amount, prices_in)?.execute()?;

    let collateral_value = 1_000 * unit;
    let collateral_amount = collateral_value / entry;
    let size_delta_usd = collateral_value * leverage;

    let mut position = TestPosition::long(true);
    let _ = position
        .ops(&mut market)
        .increase(prices_in, collateral_amount, size_delta_usd, None)?
        .execute()?;

    let prices_out = Prices::new_for_test(exit_price, exit_price, exit_price);
    let report = position
        .ops(&mut market)
        .decrease(prices_out, size_delta_usd, None, 0, flags)?
        .execute()?;

    let out = *report.output_amount() + *report.secondary_output_amount();
    let pnl = format!("{:?}", report.pnl().pnl()).parse::<i128>().unwrap_or(0);
    let uncapped = format!("{:?}", report.pnl().uncapped_pnl())
        .parse::<i128>()
        .unwrap_or(0);
    let step = format!("{:?}", report.insolvent_close_step());
    Ok((collateral_amount, out, pnl, uncapped, step))
}

#[test]
fn liquidation_never_pays_more_than_collateral() -> gmsol_model::Result<()> {
    // 10x long, price -15% => loss ~1.5x collateral => deeply underwater.
    let exit = 85_000 * PRICE_UNIT;
    let (collateral, out, pnl, _unc, step) = open_move_close(10, exit, liquidation_flags(), None, 10_000_000)?;
    println!("liquidation 10x @ -15%: collateral={collateral} out={out} pnl={pnl} insolvent_step={step}");
    assert!(
        out <= collateral,
        "liquidation paid MORE than the collateral (collateral={collateral}, out={out})"
    );
    Ok(())
}

#[test]
fn insolvent_close_pays_the_trader_nothing() -> gmsol_model::Result<()> {
    // 20x long, price -20% => loss ~4x collateral => far past insolvency.
    let exit = 80_000 * PRICE_UNIT;
    let (collateral, out, pnl, _unc, step) = open_move_close(20, exit, liquidation_flags(), None, 10_000_000)?;
    println!("insolvent 20x @ -20%: collateral={collateral} out={out} pnl={pnl} insolvent_step={step}");
    assert_eq!(
        out, 0,
        "an INSOLVENT close paid the trader {out} (collateral={collateral}) — value taken from LPs"
    );
    Ok(())
}

#[test]
fn a_healthy_position_cannot_be_liquidated() -> gmsol_model::Result<()> {
    // 5x long, price -5% => down 25% of collateral but still solvent. Forcing a liquidation here
    // would let anyone close a healthy position out from under its owner.
    let exit = 95_000 * PRICE_UNIT;
    let err = open_move_close(5, exit, liquidation_flags(), None, 10_000_000).err();
    println!("liquidating a healthy 5x @ -5% position -> {err:?}");
    assert!(
        matches!(err, Some(gmsol_model::Error::NotLiquidatable)),
        "a SOLVENT position was liquidatable (got {err:?}) — forced-close griefing"
    );
    Ok(())
}

#[test]
fn profit_is_actually_capped() -> gmsol_model::Result<()> {
    let unit = unit();
    // Cap trader PnL hard, then make a very profitable move.
    let capped = MaxPnlFactors {
        deposit: 60 * unit / 100,
        withdrawal: 60 * unit / 100,
        trader: unit / 100, // 1%
        adl: 70 * unit / 100,
    };
    let exit = 200_000 * PRICE_UNIT; // +100%
    let (collateral, out, pnl, uncapped, _step) =
        open_move_close(5, exit, Default::default(), Some(capped), 100_000)?;
    println!("capped 5x @ +100%: collateral={collateral} out={out} pnl={pnl} uncapped={uncapped}");
    assert!(
        pnl < uncapped,
        "PnL was NOT capped (pnl={pnl}, uncapped={uncapped}) — the cap does not bind"
    );
    Ok(())
}
