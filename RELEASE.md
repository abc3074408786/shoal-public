# Release checklist — velella v0.1

Everything here needs the maintainer's GitHub account; the repo already
contains everything mechanical (workflows, formula, fixtures).

## 0. Pre-flight (already done in-repo)

- [x] Name cleared: **velella** — npm/PyPI/crates.io/Homebrew/GitHub all empty
      (evidence + rejected candidates: `docs/open-source-plan.md` §4)
- [x] License Apache-2.0, CI (ci.yml), nightly compat (nightly.yml),
      release automation (release.yml), Homebrew formula (packaging/velella.rb)
- [x] 44 tests, clippy clean, release binary builds (~4 MB)

## 1. GitHub

- [ ] Create org **velella-dev** (or rename everywhere if taken)
- [ ] Create repo **velella-dev/velella**, push `main`
      (`git remote add origin git@github.com:velella-dev/velella.git && git push -u origin main`)
- [ ] Repo settings: Actions enabled; branch protection on `main`
      (require ci.yml)
- [ ] Watch the first nightly run (cron 03:00 UTC) — fixture replay must
      hold on all three OSes

## 2. v0.1.0 release

- [ ] `git tag v0.1.0 && git push origin v0.1.0` → release.yml builds and
      attaches binaries (linux-amd64, macos arm64/x64, windows-amd64)
- [ ] Release notes: auto-generated + the one-liner:
      "Run coding-agent tasks in the background, verify them automatically,
      get pinged only when needed."
- [ ] Homebrew tap: create **velella-dev/homebrew-velella**, copy
      `packaging/velella.rb` in, bump url/tag; test
      `brew install velella-dev/velella/velella`

## 3. Demo assets

- [ ] Record the demo (script below, ~40s): `vhs demo.tape` (charmbracelet/vhs)
      or a terminal screen recording → demo.gif in README
- [ ] Screenshot `velella board` with 3 tasks in flight

Demo script (deterministic, uses the fake adapter — see
`.velella/adapters/` in any smoke dir):

```bash
velella serve &                       # terminal 1
velella task run .velella/tasks/a.yaml
velella task run .velella/tasks/b.yaml
velella task run .velella/tasks/c.yaml
velella board                         # terminal 2 — record this one
```

## 4. Naming leftovers (from the naming review)

- [ ] Domain: check `velella.dev` / `velella.run` / `velella.io`
- [ ] Trademark scan: software-class "velella" (biology binomial, low risk)
- [ ] Confirm crates.io owner accounts reserved (crate name free today)

## 5. Launch posts (draft one-liners)

- Show HN: "Velella – run coding-agent tasks in the background, verify them
  automatically"
- r/ClaudeAI / r/rust: same pitch + the board GIF
- The wedge: agent-agnostic, tasks-as-code, verification-first, works with
  the CLIs you already pay for

## After launch

- Watch nightly compat: it is the product's smoke alarm
- First issues to expect: TUI quirks on exotic terminals, adapter drift
  (fix = new fixture + pattern, PRs welcome), Windows ConPTY edge cases
