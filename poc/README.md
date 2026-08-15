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

## The two mistakes this file exists to prevent

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
