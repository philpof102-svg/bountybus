# Preflight — the four gates before you hunt

Run these before spending a single agent. Three of the four can make an otherwise-real finding
**unpayable**, and you want to know that *first*.

## 1. Scope — what actually pays

Read the program's scope page and write down the **exact** in-scope impacts and their max reward.
Everything else is noise. If "theft of funds" pays and "info disclosure" does not, a finder that
hunts info disclosure is wasted budget. Feed the impacts into `args.impacts`.

Also capture the program's out-of-scope list (needs privileged access, requires a mined block,
theoretical/no-PoC, social engineering, DDoS…) into `args.outOfScope`.

## 2. Version match — is your checkout the deployed code?

A bug in old code is not payable. Confirm your clone's HEAD is the current released code.

```bash
# HEAD you cloned
git -C <repo> rev-parse --short HEAD
# latest release tag
curl -s "https://api.github.com/repos/<org>/<repo>/releases/latest" | grep tag_name
```

If HEAD is behind the latest release/branch that's actually deployed, re-clone the right ref.

## 3. Dedup — is it already known?

A disclosed/fixed bug is out of scope. Check before you invest:

```bash
# published security advisories
curl -s "https://api.github.com/repos/<org>/<repo>/security-advisories"
# your own prior submissions on this target (don't re-file your own report)
```

Also grep the changelog / recent commits for a fix that already covers your candidate.

## 4. PoC harness — can you actually prove it?

Most programs **require a runnable PoC**. Before hunting, confirm a way to run one exists — ideally
the repo's own integration harness, so you never touch mainnet (usually prohibited).

```bash
# does the repo ship a local/private-network test harness?
grep -RIl "privatenet\|privnet\|integration" <repo>/__tests__ <repo>/test 2>/dev/null
node -e 'console.log(require("<repo>/package.json").scripts)'   # look for test / test_integration
```

If there is no harness and you cannot build one, a "finding" you cannot demonstrate is not
submittable — decide that now, not after the hunt.

---

Only when all four gates are green do you run `hunt.workflow.js`. The gates are also why an honest
harness returns **zero** so often: most candidates die at gate 1 (out of scope) or gate 3 (known).
