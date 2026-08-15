# poc/ — experiments, not arguments

Reading code produces hypotheses. Running it produces facts. Everything in this directory is an
experiment that was actually executed, with its real output recorded.

## `gmsol-value-conservation.rs`

A value-conservation and liveness harness for a GMX-V2-style AMM (`gmsol-labs/gmx-solana`,
`crates/model`). Drop it in `crates/model/tests/` and run:

```
cargo test -p gmsol-model --test deployed_params -- --nocapture
```

It asks two questions the whole review hinged on:

1. **Theft** — can a deposit → withdraw round trip return more *value* than it put in?
2. **Permanent freezing** — can any accepted deposit wedge the market so an existing LP can no
   longer withdraw?

**Measured result on deployed parameters:** round trips lose **13–20 bps** (the fee), never gain;
24 escalating deposit sizes produced 14 accepted / 10 rejected (`MaxPoolValueExceeded` from 10^14 —
the ceiling working as intended), and **withdrawals stayed live after every single attempt**. No
extraction, no wedge.

## `gmsol-position-conservation.rs`

The same treatment for the surface that actually matters on a perps protocol: positions. Drop it in
`crates/model/tests/` and run:

```
cargo test -p gmsol-model --test position_conservation -- --nocapture
```

Invariants: a round trip at **unchanged price** must lose (fees are real, free money is not), and a
**losing** position must never return more than its collateral.

**Measured on deployed parameters:** round trips cost **12 / 24 / 60 / 121 bps** at 1x / 2x / 5x /
10x, identically for longs and shorts — the loss scales cleanly with size, never inverts. A long
at −20% returned 497M against 1,000M collateral; a short at +20% returned 664M. Both directions
lose when they should.

## `gmsol-liquidation-adl.rs`

The last value surface: forced closes. The earlier position harness passed
`DecreasePositionFlags::default()` (every flag false), which means the liquidation path was never
actually exercised — worth checking before believing any "positions are clean" claim.

```
cargo test -p gmsol-model --test liquidation_adl -- --nocapture
```

**Measured on deployed parameters:**

| Invariant | Result |
|---|---|
| A solvent position cannot be liquidated | `NotLiquidatable` — no forced-close griefing |
| Liquidation (10x, −15%) pays ≤ collateral | output **0** |
| Insolvent close (20x, −20%) pays nothing | output **0**, `insolvent_close_step = Some(Pnl)` — the pool absorbs the shortfall |
| PnL capping (ADL) actually binds | uncapped 4,999.9 → paid **1,999.0** units, exactly the configured 1% of pool value |

Every path that moves money on a forced close either pays zero or pays the capped amount. Nothing
leaks to the trader.

## `gmsol-time-fees.rs`

Borrowing accrual — the surface all three harnesses above miss, because they open and close inside
the same instant and never call `move_clock_forward`. A protocol can look perfectly conservative
under instant round trips and still let a trader hold leverage for free.

```
cargo test -p gmsol-model --test time_fees -- --nocapture
```

**Measured:** borrowing accrues correctly and scales linearly with pool usage —

| Pool (usage) | rate/sec | extra cost of a 1-year hold |
|---|---:|---:|
| 1,000,000u (0.5%) | 9.5e9 | 15,007,187 |
| 100,000u (5%) | 9.5e10 | 150,070,458 |
| 20,000u (25%) | 4.76e11 | 750,320,765 |

Churning 12×30d also costs strictly more than one continuous 360d hold, so leverage cannot be held
for free by cycling.

### The trap this file walked into, and what it exposed

`TestMarket` initialises each clock **lazily**: `clocks.entry(kind).or_insert(now)`, so the *first*
`just_passed_in_seconds(ClockKind::Borrowing)` call always reports **0 seconds elapsed**, however
much time has passed. A test that moves the clock and then updates once measures exactly zero
borrowing. Production does not have this problem — `programs/store`'s `update_fees_state()` runs
before *every* order ("[Pre-execute] borrowing state updated"), so the clock is live from the
opening order onward. Modelling both orders makes the fees appear, as the table shows.

But the same trap catches the crate's own suite. Running it unmodified:

```
$ cargo test -p gmsol-model --test borrowing_fee -- --nocapture
test_total_borrowing_with_high_borrowing_factor   -> TestPool { long_amount: 0, short_amount: 0 }
test_total_borrowing_with_high_borrowing_factor_2 -> cumulative borrowing factor: 0
test result: ok. 2 passed
```

Both tests advance the clock (one by **100 years**), both print **zero** borrowing, and both pass —
they `println!` and never assert, exactly like `round_attack_deposit`. Two tests named for a high
borrowing factor measure none of it.

## The three mistakes these files exist to prevent

Both were made here, and both would have produced a confident, wrong "critical finding".

**1. Fixture constants are not deployed constants.** `TestMarketConfig::default()` ships
`fee_receiver_factor = 0.37` and swap-impact factors ~2.3× the real ones, and a
`max_pool_value_for_deposit` ceiling ~8 orders of magnitude higher than production. An earlier
candidate's profitability was computed on the fixture and inverted once
`scripts/resources/config/market_configs.toml` values were used. Copy the deployed numbers in
explicitly, with the file and line you took them from.

**2. Two token amounts are not one number.** A withdrawal returns *both* long and short tokens, and
with `Prices::new_for_test(120, 120, 1)` a long token is worth 120 and a short token 1. Summing the
raw amounts manufactured a fake **+71% "profit"** and a failing assertion that looked exactly like a
critical bug. Compare **value**, never token counts — and when a result says free money, suspect
your units before you suspect the protocol.

**3. A boolean is not a direction.** `TestPosition::long(x)` / `::short(x)` — the parameter `x` is
*which token is used as collateral*, not the position's side. `long(false)` is still a **long**.
Testing "shorts" that way produced a short that *profited* when the price rose, with a positive PnL:
a textbook sign bug, except the bug was in the test. Read the constructor before trusting the
assertion it feeds.

**4. A guard firing is not a bug, and a cap that does not bind may just be badly parameterised.**
Trying to liquidate a healthy position returned `NotLiquidatable` — the protocol correctly refusing,
which is itself an invariant worth asserting. Separately, a PnL cap set to 1% appeared not to bind:
the pool was 100× larger than the position, so 1% of pool value exceeded the profit. Shrinking the
pool made the cap bind exactly as configured. Before calling a control broken, check that your
scenario actually reaches it.

The pattern across all four: every one produced a *failing assertion that looked like a critical
finding*. A harness that reports the first red test as a vulnerability is a false-positive machine.
Reproduce, then attack your own instrumentation, then report.
