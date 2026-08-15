# Courtesy note to gmsol-labs — GM share accounting at minimal supply

**Not a bug bounty submission.** This does not map to a payable impact under the GMTrade program
(Primacy of Rules), and we are not filing it on Immunefi. No funds were touched; all work was static
analysis plus a local test run on a public checkout of `gmsol-labs/gmx-solana` at `main`.

The mechanism below is one you already know about — you have a test named for it. The part we think
is genuinely worth your attention is that **that test asserts nothing** (see "the part that may
actually be worth your time").

## What we observed

Two behaviours combine at minimal market-token supply:

1. **Withdrawal leaves the pool-side fee in the pool.** In `crates/model/src/action/withdraw.rs`
   the pool delta applied is `-(fee_amount_for_receiver + amount)` — `Fees::fee_amount_for_pool`
   (`crates/model/src/params/fee.rs`) intentionally stays behind — while the *full* requested
   `market_token_amount` is burned.

2. **Mint uses floor division.** `crates/model/src/utils.rs` computes the minted amount as a plain
   floor `supply.checked_mul_div(&usd_value, &pool_value)`, reached from
   `crates/model/src/action/deposit.rs`.

Consequently a sole LP who repeatedly deposits and withdraws can drive `supply` toward 1 raw unit
while pool value retains the accumulated fee residue. At `supply == 1`, any subsequent deposit with
`usd_value < pool_value` mints **0** tokens, and the deposited tokens are still credited to the pool.

## Why we did not report it as a vulnerability

We tried to refute it, and it does not survive as a payable finding:

- **A working on-chain guard exists.** `DepositActionParams::validate_market_token_amount`
  (`programs/store/src/states/deposit.rs`) enforces
  `require_gte!(minted, min_market_token_amount)`. A depositor with any sane minimum is protected;
  the loss path requires `min_market_token_amount == 0`.
- **A purpose-built mitigation exists.** `validate_first_deposit`
  (`programs/store/src/states/deposit.rs`) pins the first deposit to a designated
  `first_deposit_receiver` and enforces `MinTokensForFirstDeposit` — the standard defence against
  this vault-inflation class, deliberately enforced at the store layer.
- **The economics do not work on deployed parameters.** An early profitability estimate used the
  test fixture's `fee_receiver_factor` of `0.37` (`crates/model/src/test.rs`), whereas the deployed
  config uses `swap_fee_receiver_factor = 0.70`
  (`scripts/resources/config/market_configs.toml`). With 70% of the fee leaving to the receiver,
  the residue an attacker can accumulate is less than half of what the fixture implies, and the cost
  of churning capital exceeds the size of the rounding wall it creates.

## The part that may actually be worth your time

You are clearly already aware of this class — `crates/model/src/action/deposit.rs` contains a test
named `round_attack_deposit`. **But that test has no assertions.** It deposits `1` ten million times,
`println!`s the market, deposits `10_000_000 - 1` into a second market, `println!`s that, and returns
`Ok(())`. It passes regardless of what the accounting does, so it cannot catch a regression in the
behaviour it is named after — while costing ~90s of every test run.

Turning it into a real check (assert the two markets' supply/pool value agree within a stated
tolerance) would convert ~90 seconds of unconditional green into an actual invariant. That is the
one concrete thing we would suggest.

Verified locally: `cargo test -p gmsol-model --lib` → `33 passed; 0 failed` (Linux toolchain,
`rustc` 1.97.1).

## Optional hardening, if you think it is worth it

- Round the minted amount consistently with the burn side, or reject a deposit that would mint `0`
  outright rather than crediting the tokens to the pool.
- Consider enforcing a minimum non-zero supply invariant on withdrawal, so supply cannot be driven
  to 1 while value remains.

Neither is urgent given the guards above. Sharing in case it is useful.

---

*Found with [bountybus](https://github.com/philpof102-svg/bountybus), an adversarial multi-agent
review harness — the same harness refuted it. Reported here rather than on Immunefi precisely
because it did not survive that refutation.*
