# Example run — GMTrade / gmx-solana (2026-08)

Four passes against a young target (live ~1 month), Rust/Anchor on Solana. Program: GMTrade on
Immunefi — up to $100k critical, **Primacy of Rules**, runnable PoC required. Kept as the reference
example because it shows the harness doing the two things that actually matter: finding a real
mechanism, and then **killing it for the right reasons**.

## Preflight

- **Scope**: `programs/store`, `programs/treasury`, `programs/liquidity-provider` + the deployed
  verified-builds. Payable impacts: theft of funds, insolvency, permanent/temporary freezing, theft
  of unclaimed yield.
- **Dedup**: the repo publishes a `## Known Issues` section and a separate audits repo — both were
  fed to every finder as out-of-scope, so candidates could not rediscover documented behaviour.
- **Version match**: `main` HEAD, matching the live deployment window. ✓
- **PoC harness**: `cargo test` on the workspace. On Windows this fails for lack of an MSVC linker —
  the working path is a Linux toolchain (WSL: `rustc`/`cargo`/`gcc`), building on a native Linux
  filesystem rather than `/mnt/c`.

## The four passes

| Pass | Target | Result |
|---|---|---|
| 1 | `programs/store` (broad: account validation, PnL, GM pricing, swap/oracle, treasury, LP) | 1 candidate → refuted |
| 2 | *(mis-targeted — see below)* | invalid |
| 3 | `store/src/ops`, `states/market`, `instructions/exchange/execute_*` | 0 |
| 4 | `crates/model` (the pure math, 12.4k LOC) | 1 candidate → refuted |

**Pass 1's candidate** — reward APY averaged over stake age but applied per incremental claim, so
splitting claims mints more GT than the schedule intends. Refuted: GT is *freshly minted per user*
with no shared pool, so no other LP loses claimable yield and nothing is drained. The effect is
dilution of a reward token — an economic consequence, not a payable impact.

**Pass 4's candidate** — at minimal supply, withdrawal leaves `fee_amount_for_pool` in the pool
while burning the full requested amount; combined with floor division on mint, a sole LP can drive
supply to 1 raw unit and zero out the next depositor's mint. Refuted on three independently
verified pillars: a working on-chain `min_market_token_amount` guard, a purpose-built
`validate_first_deposit` mitigation, and — decisively — profitability that was computed with the
**test fixture's** `fee_receiver_factor = 0.37` instead of the **deployed** `0.70`, which inverts
the trade. See [gmsol-courtesy-note.md](gmsol-courtesy-note.md) for what we sent the project anyway.

## The lesson that changed the harness

**Pass 2 was a false clean.** Four of its six finders were pointed at guessed sub-paths
(`store/src/exchange`, `/glv`, `/migration`) that do not exist — the real layout is
`store/src/instructions/exchange` and `store/src/ops`. They returned "no findings", which is
indistinguishable from a clean audit. Nothing in the output revealed it.

That is the worst failure mode an audit harness can have, and it is now guarded: every finder lists
its target first and reports `dir_ok`, and the synthesize step emits `misTargeted` **louder than the
findings**. Verify your paths before you trust a zero — and note that pass 3, run on verified paths,
still returned zero, which is what makes that zero meaningful.

## The most useful thing the run produced

Not a vulnerability — a **dead test**. Building the crate on the Linux toolchain
(`cargo test -p gmsol-model --lib` → `33 passed; 0 failed`) surfaced
`action::deposit::tests::round_attack_deposit`, a test named for exactly the mechanism pass 4 found.
Reading it: it deposits `1` ten million times, `println!`s the market, deposits `10_000_000 - 1`
into a second market, `println!`s that, and returns `Ok(())`. **No assertions.** It passes whatever
the accounting does, and costs ~90s of every run.

That is worth more to the project than the refuted finding was, and it only appeared because the
harness was actually built and run — not read. Reported as a courtesy note, not a bounty.

## Pass 5 — measuring instead of reading

With the toolchain unblocked, the deposit/withdraw math was tested rather than argued: a
value-conservation and liveness harness ([`poc/gmsol-value-conservation.rs`](../poc/gmsol-value-conservation.rs))
built on the **deployed** constants from `market_configs.toml`, not the crate's fixture.

Measured: round trips lose **13–20 bps** (the fee) and never gain; across 24 escalating deposit
sizes, 14 were accepted and 10 rejected by the `max_pool_value` ceiling, and **an existing LP could
still withdraw after every single attempt**. So neither theft nor permanent freezing is reachable
through that path — a much stronger statement than "we read it and it looked fine".

Getting there took two wrong turns worth recording, because each briefly looked like a critical
finding: summing long and short token amounts as if they shared a price produced a fake **+71%
round-trip profit**, and running on the fixture's `fee_receiver_factor = 0.37` instead of the
deployed `0.70` had already inverted an earlier profitability estimate. Both are written up in
[`poc/README.md`](../poc/README.md).

## Takeaway

Two real mechanisms surfaced across ~2.7M tokens of review; both died in adversarial verification,
for reasons that were checked by hand afterwards and held. Zero payable findings on a young,
actively-developed protocol is an ordinary outcome — and a harness that had reported either
candidate as "critical" would have burned the reporter's standing on the platform.
