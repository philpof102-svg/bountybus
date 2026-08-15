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

The pattern across all three: every one produced a *failing assertion that looked like a critical
finding*. A harness that reports the first red test as a vulnerability is a false-positive machine.
Reproduce, then attack your own instrumentation, then report.
