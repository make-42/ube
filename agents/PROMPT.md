# Prompts for agents working on ube engines

Copy one block, fill the `{{...}}` placeholders, paste it to the agent. Every prompt assumes the agent can read this repository; if it also has a shell with network access, it should clone SearXNG as shown. If it has no network, it must **ask you to run the probes** (see `ENGINES.md` section 11) instead of guessing.

One-time setup for the agent's sandbox:
```sh
git clone --depth 1 https://github.com/searxng/searxng.git /tmp/searxng
```

---

## Prompt A: port one SearXNG engine

```text
You are working in the `ube` repository (a Rust metasearch server: axum + scraper + reqwest/wreq).

TASK
Port the SearXNG engine `{{SEARXNG_MODULE}}` (instance name(s): {{SEARXNG_INSTANCES}}) to ube as a new source for the `{{KIND}}` tab. Backlog row: "{{BACKLOG_ROW_NAME}}" in agents/BACKLOG.md.

READ FIRST, IN THIS ORDER (do not skip, do not skim)
1. agents/ENGINES.md, entirely. Its rules override your habits.
2. The backlog row for this engine in agents/BACKLOG.md (types, particularities, effort).
3. The SearXNG source: `python3 agents/tools/sx_engine.py /tmp/searxng {{SEARXNG_MODULE}}`. Also read every module it lists under "SHARES CODE WITH".
4. The reference engines: src/sources/engines/dockerhub.rs (JSON) or bing_videos.rs (HTML), and the registry file src/sources/{{KIND}}.rs.

PROCEDURE
1. `git status` must be clean. If not, stop and report what is modified.
2. Work out the engine type(s) from ENGINES.md section 8 and apply that type's checklist.
3. Get real evidence of what the live site returns for page 1 AND page 2 (ENGINES.md section 11). If you cannot reach the site, STOP after writing the exact probe commands and ask me to run them and paste the full output. Do not write selectors or JSON paths from memory or from SearXNG's code alone.
4. Write src/sources/engines/{{ENGINE_FILE}}.rs following the skeleton in ENGINES.md: pure `build_url` and `parse`, `x_pages` + `x_with` entry points, a Config struct only if region/language/instance/time range matters, a zero-results diagnostic log, tolerant JSON parsing, `Err` for blocks.
5. Tests: fixtures copied from the real responses you were given; scores across pages; build_url; defensive cases. For any bug you fix, prove the regression test fails without the fix.
6. Register it in src/sources/engines/mod.rs and src/sources/{{KIND}}.rs with weight {{WEIGHT}} (see ENGINES.md section 13).
7. `cargo test` with zero warnings. Commit as `feat({{ENGINE_FILE}}): ...`, then `git format-patch -1`, then verify the patch with `git am` on a clean checkout of the base.

HARD RULES
- No new dependency, no new `Kind`, no change to static/search.html without asking me first.
- No API keys in code; env vars only.
- Do not try to defeat Anubis, captchas, DataDome or similar. Report them as blocks.
- Do not modify unrelated code. Do not keep code you did not write and cannot explain.
- Do not claim it works against the live site unless you have live evidence.

DELIVER
The patch file, then a short message: what it does (endpoint, paging, extras), differences from SearXNG, what is NOT verified, and the exact command for me to run if you need more evidence.
```

---

## Prompt B: fix an engine that is broken or returns wrong data

Use this when you ran the engine and something is off (nothing shows up, titles missing, page 2 empty...).

```text
You are working in the `ube` repository. Read agents/ENGINES.md entirely before anything else.

PROBLEM
Engine: {{ENGINE_FILE}} (source name `{{SOURCE_NAME}}`, kind {{KIND}}).
Symptom: {{SYMPTOM, e.g. "page 2 shows no results", "titles are empty", "0 results parsed"}}.
Evidence I collected (verbatim, do not reinterpret):
<<<
{{PASTE: the server log line (e.g. the `0 results parsed ...` warning), probe output, an item dump, or the rendered DOM/text}}
>>>

PROCEDURE
1. `git status` must be clean; if not, stop and report.
2. Explain the cause in 3 to 5 sentences, citing which line of my evidence shows it. Use the lookup table in ENGINES.md section 11 ("Reading the results") and the lessons in section 7 (noscript, content-based lookup, merged blobs, page 2 differs).
3. If the evidence does NOT identify the cause, do not guess a fix. Write the single most informative probe command (ENGINES.md section 11) and ask me to run it. Stop.
4. If it does: add a regression test using a fixture copied from my evidence, prove that it fails without the fix (show the failing assertion), then fix, then run the whole suite.
5. Commit as `fix({{ENGINE_FILE}}): ...`, `git format-patch`, verify with `git am` on a clean base (stack it after the earlier patches in the series).

DELIVER
The patch, the cause, the proof the test fails without the fix, and what is still unverified.
```

---

## Prompt C: take the next engines from the backlog

```text
You are working in the `ube` repository. Read agents/ENGINES.md entirely, then agents/BACKLOG.md.

TASK
Pick the next {{N}} engines to port: go down the main table in agents/BACKLOG.md from the top, skip rows already ported (check src/sources/engines/ and the registries) and rows marked BLOCKED, and prefer rows that need no Phase 0 foundation that does not exist yet. Phase 0 foundations (F1 to F8) are separate patches and come first when a row depends on one; tell me which one before starting it, and ask before adding any dependency (F7).

For EACH engine, follow Prompt A's procedure exactly (evidence first, one patch per engine, stacked in order). If the live site is not reachable for an engine, do not skip the evidence step: write its probe commands, ask me to run them, and meanwhile continue with the next engine whose evidence you can already obtain.

Stop after {{N}} patches (or earlier if blocked) and report: the order you chose and why, per-patch summaries, what is unverified, and which rows should come next.
```

---

## Prompt D: build a shared foundation (F1 to F8 in BACKLOG.md)

```text
You are working in the `ube` repository. Read agents/ENGINES.md entirely, then the "Phase 0" table in agents/BACKLOG.md.

TASK
Implement foundation {{F#}} ("{{FOUNDATION NAME}}"). Requirements:
- It must not change the behaviour of any existing engine. If you move code (for example `strip_noscript` out of bing_videos.rs), the existing tests must pass unchanged and still prove the same behaviour.
- Minimal API, documented with a short module comment and unit tests.
- No new dependency unless I approved it in this conversation: if it needs one (XML parser, date crate), stop and ask me, naming the crate, why, and what it costs.
- One patch, `refactor(...)` or `feat(...)`, verified with `git am` on a clean base.
- Modify BACKLOG.md to specify that the foundation is implemented.

DELIVER
The patch and a list of the engines in BACKLOG.md that this unlocks.
```
