export const meta = {
  name: 'bountybus-hunt',
  description: 'Adversarial multi-agent bug-bounty pass: fan out finders across the attack surface, then try to refute every candidate before it survives.',
  phases: [
    { title: 'Hunt', detail: 'one finder per attack surface, scope-locked' },
    { title: 'Verify', detail: 'one adversarial refuter per candidate finding' },
    { title: 'Synthesize', detail: 'rank survivors, list every reject with its reason' },
  ],
}

// ── config, all via args (nothing hardcoded) ─────────────────────────────────────────────────────
// args = {
//   impacts: string[]            // the ONLY impacts that pay for this program, e.g. ['rce','theft_funds']
//   outOfScope?: string          // extra program-specific out-of-scope text
//   context?: string             // one paragraph: what the target is, where untrusted input enters
//   finders: [{ key, dir, brief }]  // one per attack surface. dir = an already-cloned local path.
//   maxFindingsPerFinder?: number   // default 3
// }
// args may arrive as a parsed object (inline run) or a JSON string (scriptPath run) — accept both.
const A = (typeof args === 'string') ? JSON.parse(args) : (args || {})
const IMPACTS = A.impacts || ['rce', 'theft_funds', 'theft_nfts']
const FINDERS = A.finders || []
const MAX = A.maxFindingsPerFinder || 3
if (!FINDERS.length) throw new Error('bountybus: args.finders is required (one {key,dir,brief} per attack surface)')

const SCOPE = `
BOUNTY SCOPE — only these impacts count as PAYABLE, everything else is OUT OF SCOPE:
  ${IMPACTS.map((i) => `- ${i}`).join('\n  ')}
${A.context ? `\nTARGET CONTEXT:\n${A.context}` : ''}
GENERIC OUT OF SCOPE (do NOT report, or mark in_scope=false): anything needing privileged access,
operator config-file control, leaked keys, social engineering/phishing, pure DDoS, theoretical
side-channels with no runnable PoC, self-XSS, missing headers, best-practice notes, or any impact
that requires the victim to perform an un-prompted action outside the normal workflow.
${A.outOfScope ? `PROGRAM-SPECIFIC OUT OF SCOPE:\n${A.outOfScope}` : ''}
A finding is only worth reporting if a REMOTE or LOCAL-UNPRIVILEGED attacker with realistic access
can reach it AND it maps to one of the payable impacts above WITH a runnable PoC.`

const FINDINGS_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    // dir_ok exists because of a real failure: a finder pointed at a path that did not exist returned
    // "no findings" — indistinguishable from a clean audit. A mis-targeted finder must be LOUD.
    dir_ok: { type: 'boolean', description: 'true only if the target directory exists and contains the code you were asked to read' },
    reviewed: { type: 'string', description: 'the files/areas you actually read, so a null result is auditable' },
    findings: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        properties: {
          title: { type: 'string' },
          file: { type: 'string' }, line: { type: 'integer' },
          impact: { type: 'string' },
          attacker_input: { type: 'string', description: 'exact attacker-controlled input and how it reaches the code' },
          data_flow: { type: 'string', description: 'source -> ... -> sink, with real function names you read' },
          why_exploitable: { type: 'string' },
          poc_sketch: { type: 'string' },
          confidence: { type: 'string', enum: ['low', 'medium', 'high'] },
        },
        required: ['title', 'file', 'impact', 'attacker_input', 'data_flow', 'why_exploitable', 'poc_sketch', 'confidence'],
      },
    },
  },
  required: ['dir_ok', 'reviewed', 'findings'],
}

const VERDICT_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    real: { type: 'boolean' },
    in_scope: { type: 'boolean' },
    severity: { type: 'string', enum: ['critical', 'high', 'medium', 'low', 'none'] },
    refutation_attempt: { type: 'string', description: 'the strongest case AGAINST this being real + in-scope' },
    upstream_guard: { type: 'string', description: 'any validation that blocks the path, with file:line, or "none found"' },
    poc_feasible: { type: 'boolean' },
    verdict_reason: { type: 'string' },
  },
  required: ['real', 'in_scope', 'severity', 'refutation_attempt', 'upstream_guard', 'poc_feasible', 'verdict_reason'],
}

phase('Hunt')
const results = await pipeline(
  FINDERS,
  (f) => agent(
    `You are a senior security auditor hunting a live bug bounty. ${SCOPE}\n\nTASK: ${f.brief}\n\n`
    + `The code is already cloned locally. Read it with Grep/Read/Bash under: ${f.dir}\n`
    + `Only report a finding if you can name the attacker input, trace it to the sink with real file:line `
    + `you actually read, and sketch a runnable PoC. Returning an EMPTY findings array is a CORRECT and `
    + `expected outcome — do not manufacture issues. Always fill "reviewed" so a null result is auditable. `
    + `FIRST: list the target directory and confirm it contains the code named in your task. Set dir_ok=false `
    + `and say so in "reviewed" if the path is missing/empty — a finder that reviewed nothing must NOT look `
    + `like a clean audit. Return at most your ${MAX} strongest findings.`,
    { label: `hunt:${f.key}`, phase: 'Hunt', schema: FINDINGS_SCHEMA, agentType: 'general-purpose', effort: 'high' }
  ).then((r) => ({ finder: f.key, dir: f.dir, dirOk: !!(r && r.dir_ok), reviewed: (r && r.reviewed) || '', findings: (r && r.findings) || [] })),
  // Keep the finder's own metadata alongside the verified findings — returning a bare array here is what
  // made `coverage` permanently read "(see journal)" and hid mis-targeted finders.
  async (res) => ({
    ...res,
    verified: await parallel(
      (res.findings || []).slice(0, MAX).map((fd) => () =>
        agent(
          `You are an adversarial verifier trying to REFUTE a bug-bounty finding, not confirm it. ${SCOPE}\n\n`
          + `CANDIDATE (repo dir ${res.dir}):\n${JSON.stringify(fd, null, 2)}\n\n`
          + `Re-read the actual code at ${res.dir} yourself. KILL this finding if it is not real or not in scope. `
          + `Check: (a) is attacker_input truly controllable by a realistic remote/local-unprivileged actor, or does `
          + `it need operator/config/privileged access (=> out of scope)? (b) is there an upstream guard — quote it `
          + `with file:line? (c) does it truly map to a payable impact with a runnable PoC, or is it theoretical? `
          + `Default to real=false when in doubt.`,
          { label: `verify:${fd.file || fd.title}`, phase: 'Verify', schema: VERDICT_SCHEMA, agentType: 'general-purpose', effort: 'high' }
        ).then((v) => ({ ...fd, finder: res.finder, verdict: v }))
      )
    ),
  })
)

phase('Synthesize')
const runs = results.filter(Boolean)
const all = runs.flatMap((r) => (r.verified || []).filter(Boolean))
const survivors = all.filter((f) => f.verdict && f.verdict.real && f.verdict.in_scope && f.verdict.poc_feasible)
const rejected = all.filter((f) => !(f.verdict && f.verdict.real && f.verdict.in_scope && f.verdict.poc_feasible))
const misTargeted = runs.filter((r) => !r.dirOk).map((r) => ({ finder: r.finder, dir: r.dir, note: r.reviewed }))
const rank = { critical: 4, high: 3, medium: 2, low: 1, none: 0 }

log(`bountybus: ${all.length} candidates, ${survivors.length} survived adversarial verification.`)
// A mis-targeted finder is a FALSE CLEAN — surface it louder than the findings themselves.
if (misTargeted.length) log(`⚠ ${misTargeted.length} finder(s) could not read their target dir — their "no findings" means NOTHING: ${misTargeted.map((m) => m.finder).join(', ')}`)
if (!survivors.length && !misTargeted.length) log('Zero survivors is a valid, honest result on a mature target — not a failure of the pass.')

return {
  survivors: survivors.sort((a, b) => (rank[b.verdict.severity] || 0) - (rank[a.verdict.severity] || 0)),
  rejected: rejected.map((f) => ({ title: f.title, file: f.file, why_rejected: f.verdict ? f.verdict.verdict_reason : 'no verdict' })),
  misTargeted,
  coverage: runs.map((r) => ({ finder: r.finder, dir_ok: r.dirOk, reviewed: r.reviewed || '(empty)' })),
  counts: { candidates: all.length, survivors: survivors.length, rejected: rejected.length, misTargeted: misTargeted.length },
}
