# Hidden terminal cadence — acceptance, 2026-10-27

## User-visible result and scope

Installed terminal tabs now batch caught-up background output instead of polling
at foreground cadence. Switching back wakes the reader immediately. The scoped
change is implemented, independently reviewed, integrated, deployed and verified
in the installed UI. Everyday user-perceived multi-session UAT and CPU percentage
measurements are separate and have not been performed.

Only the hidden, no-backlog periodic delay in `terminal-output.ts` changed: at
least 250 ms between reads. Existing idle backoff, serialized in-flight work,
explicit wake, immediate backlog draining, foreground stream responsiveness and
dispose behavior are retained. This is tab visibility, not window minimization.
The runtime still records all bytes independently of viewers; no replay or
protocol-reply semantics were changed.

## Development verification

The new production-scheduler regression failed on the saved baseline with 1,251
reads in ten virtual seconds, then passed with 41 including the initial read.
All six scheduler tests passed, including existing foreground, backlog, in-flight,
reveal and disposal assertions. The visible 80 ms stream test retained 4 ms
observed sampling latencies.

An actual TerminalView browser comparison used the saved pre-change source and
the candidate under the same 20 ms producer / 128 KiB page conditions:

| Metric | Baseline | Candidate |
| --- | ---: | ---: |
| Hidden reads in a 3-second measurement window | 192 | 11 |
| Produced Unicode/ANSI lines, including setup time | 153 | 154 |
| Exact ordered rendered history after reveal | pass | pass |
| Reveal to final marker | 44 ms | 85 ms |
| Input to echo after reveal | 28 ms | 39 ms |

Request count fell 94.27% in this fixture. Both histories matched their own complete
producer sequence; small setup timing differences account for the line-count
difference. These numbers do not establish CPU savings or latency guarantees.
The browser comparison is one active and one hidden viewer, not a system-wide
multi-session CPU benchmark. Build/type checks and scoped diff checks passed;
the existing bundle-size warning remains.

Evidence: `artifacts/terminal-background-cadence/baseline-test.log`,
`scheduler-test.log`, `browser-test.log`, `browser-result.json`, `build.log`,
`baseline-terminal-output.ts` and `source-hashes.json`.

## Independent review and freeze

The read-only `background_cadence_review` reviewer compared the candidate with
the saved source baseline, independently ran all six scheduler tests, inspected
the browser test/evidence and found no blocking defect. The accepted source and
package were then frozen. No unrelated dirty-worktree changes were reverted.

## Installation and online verification

Installed executable:
`D:/orcas/artifacts/installed-no-codex-chat-20261027/Velella.exe`.
The package changes only the rebuilt renderer; all 14 installed non-renderer
files were verified byte-identical. The runtime, clipboard/export helpers,
scheduling fix, original daemon/session process identities and history prefixes
were preserved. Daemon PID remained 31828.

- App SHA-256: `c45e6efa7124c9933f745a647f41c09be848303e965573c4087e5e84367baf41`
- Runtime SHA-256: `01c24d59951e96c204b41e02a58059bb0b18ca1157fc31910cb557f0966a123b`
- Installation receipt: `artifacts/terminal-background-cadence/installation.json`
- Rollback backup: `artifacts/terminal-background-cadence/backup/`
- Receipt status: `installed_verified` for this scoped change.

In owned PowerShell session `01a17e06-e385-703c-9128-ae071dae853b`, a finite command
printed 200 numbered Chinese lines with 20 ms sleeps. The UI switched to the
original tab while output continued. Switching back showed the last line and
prompt. Exported timing data demonstrated 21 sustained background read gaps of
253.2–274 ms. Parsing covered the full 3,586-byte captured output range without
gaps, and that range contained all 200 rows in order.

The initial evidence check inadvertently included later ConPTY redraw bytes caused
by the export-status UI resizing the viewport, yielding repeated old rows. The
corrected check uses the exact exported trace boundary and passed. This was an
evidence-boundary correction, not a product retry or a change to the candidate.

Evidence: `installed-performance.json`, `native-acceptance.json`, `finalize.cjs`,
`owned.json` and `sessions-before.json` under the same artifact directory.
Only the owned test process was stopped and its tab closed. All five original
active workspace session identities were preserved; no model prompt was sent.

## Practical limits

Background live parsing can lag production by roughly the batching interval plus
transport/processing time. Reveal bypasses the wait. Large backlogs still drain
without this cap; the read-reduction claim applies only once caught up. The
change does not claim to solve every possible terminal performance issue, and
the earlier startup improvement remains separately user-accepted.
