# Keyboard startup sampling — 2026-10-27, Asia/Shanghai

Three native keyboard launches now have a complete timing export. Claude welcome
screen data arrived 1.16–1.71 seconds after Return. No transport failure or
ten-second internal backlog was observed. This does not establish physical
display latency or exclude intermittent delays under the user's normal conditions.

Keep the accepted implementation frozen: this run adds evidence, not another code
change or deployment. Increased polling or rewritten rendering has no measured
large benefit in this sample. Further work needs a captured slow run or a
separately justified investigation of pre-screen CLI/OS startup work. The user
subsequently confirmed “现在明显快了”, closing qualitative user acceptance for
this optimization round. This does not establish a maximum latency guarantee.

## Method and results

Installed app SHA-256 was verified as
`dcade3aa6efc54a09dd7d65854ba9ee434ed154e8671c3e74715c876829f5dda`.
The existing runtime and AppStore workspace were used. A new owned PowerShell
session, `01a17dea-1255-75dc-9a8a-88b584e46d47`, isolated testing from user sessions.

The first letter sequence displayed a Chinese IME candidate; it was cancelled
with Escape and the IME temporarily switched to English. Each measured launch
used individual native `c`, `l`, `a`, `u`, `d`, `e` key presses and one Return.
No clipboard paste or programmatic terminal input was used for measured commands.
Only empty owned Claude launches were exited between samples. No model prompt,
trust approval, login or global Claude setting change was made.

The installed menu exported 392 metadata entries with zero dropped entries.
Analysis matched output offsets to Claude welcome-screen markers in the owned
session byte stream, normalizing ANSI controls and spaces. Raw output was used
only in memory; it was not added to the performance export.

| Run | Input queue | Input acknowledgement | Welcome data after Return | Render callback after welcome data | Later frame opportunity after Return |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.1 ms | 13.1 ms | 1,712.4 ms | 31.1 ms | 1,757.4 ms |
| 2 | 0.1 ms | 23.0 ms | 1,253.4 ms | 2.5 ms | 1,288.4 ms |
| 3 | 0.1 ms | 12.8 ms | 1,155.4 ms | 8.9 ms | 1,178.2 ms |

All three welcome screens were observed visibly. Screenshot observations were
separated by tool/model round trips, so those intervals are not treated as startup
duration. Frame opportunity is not a physical screen-present timestamp. Three
launches in one running shell are not a cold-boot benchmark or a representative
distribution across system loads.

## Cleanup and delivery states

Original Chinese IME mode was restored and its candidate UI verified; the check
character was cancelled. Only the owned session was stopped and its tab closed.
All five originally active workspace session identities were preserved.

- Implementation: unchanged accepted candidate.
- Development validation: native keyboard samples and byte-offset analysis complete.
- Independent testing: no new independent reviewer for this measurement-only run;
  the previous export implementation review remains applicable.
- Technical acceptance: sampling complete within the stated limits.
- Integration/deployment: no new package or restart required.
- Online: installed candidate retained, original sessions preserved.
- User UAT: passed for perceived improvement. On 2026-10-27 the user answered
  “现在明显快了” to the follow-up about manually typing `claude` and pressing Return.
  No numeric manual timing or claim that every future launch stays below ten
  seconds was supplied. Keep the accepted candidate frozen.

Evidence: `artifacts/terminal-keyboard-startup/performance.json`, `result.json`,
`analyze.cjs`, `owned.json`, `before.json` and `cleanup.json`.
