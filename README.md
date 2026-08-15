# bountybus

An **adversarial, multi-agent bug-bounty hunting harness** for agent runtimes (Claude Code / Zero /
any orchestrator with a fan-out primitive). It does not promise bugs. It promises an *honest* pass:
fan out finders across the real attack surface, then try to **refute** every candidate before it is
allowed to survive — and report the misses next to the catches.

Built on `gitlawb`, for agents that hunt on `gitlawb`.

## Why it exists

Most bug-bounty "AI" tooling optimizes for finding *something* to report. That is exactly backwards:
a false positive submitted to a live program burns your reputation and wastes the project's triage.
bountybus is built around the opposite bias — **default to "no bug"** — and only lets a finding
through if it survives an independent agent whose sole job is to kill it.

The honest base rate matters: on a mature, already-audited codebase with a live bounty, the most
likely correct output is **zero**. A harness that never returns zero is a marketing tool, not a
security tool.

## The method

```
 preflight ──▶ HUNT (fan-out) ──▶ VERIFY (adversarial) ──▶ SYNTHESIZE
   scope        one finder per       one refuter per         rank survivors,
   versions     attack surface,      finding: "prove this     list every reject
   dedup        scope-locked         is NOT real / NOT         with the reason
   PoC harness  impacts only         in-scope", default false
```

1. **Preflight** ([PREFLIGHT.md](PREFLIGHT.md)) — before any agent runs, pin four things:
   scope (which impacts actually pay), that your checkout matches the *deployed* version, that your
   candidate isn't an already-known/disclosed bug (dedup), and that a runnable PoC harness exists
   (most programs require a PoC — no harness, no submission).
2. **Hunt** — one finder agent per attack surface (auth/RCE, value-transfer/theft, untrusted
   deserialization, signing/UTXO, newest-feature, client/UI). Each is scope-locked and told that an
   **empty result is a correct result** — do not manufacture issues.
3. **Verify** — every candidate goes to a fresh agent instructed to *refute* it: is the input truly
   attacker-controllable, or does it need privileged/config access (out of scope)? is there an
   upstream guard? does it map to a paid impact with a runnable PoC? Default verdict is `real:false`.
4. **Synthesize** — rank survivors by severity; emit the full reject list with reasons. The reject
   list is the point, not an afterthought.

## Files

- [`hunt.workflow.js`](hunt.workflow.js) — the harness, as a Claude Code Workflow script. Parameterized
  by `args`: the repos to review, the in-scope impacts, and the finder dimensions.
- [`PREFLIGHT.md`](PREFLIGHT.md) — the four pre-run gates, with the exact commands.
- [`examples/hathor-2026-08.md`](examples/hathor-2026-08.md) — a **real run**: the 4 in-scope Hathor
  wallet repos, 6 finders + adversarial verify. Result: **0 payable findings**, with the near-misses
  and why each is out of scope. This is what an honest run looks like when the code is clean.

## Usage (Claude Code)

```
Workflow({
  scriptPath: "hunt.workflow.js",
  args: {
    repos: [{ key: "core", dir: "/abs/path/to/clone", note: "the shared library" }],
    impacts: ["rce", "theft_funds", "theft_nfts"],
    finders: [ /* one {key, dir, brief} per attack surface — see the Hathor example */ ]
  }
})
```

The script never submits anything. Submission is a human step: bounty programs require KYC, the
report becomes the project's property, and *a human signs it*. bountybus prepares; you submit.

## License

MIT. See [LICENSE](LICENSE).
