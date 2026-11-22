# Long-session stress and memory analysis — 2026-10-28

## Result and decision

The authorized finite-volume/lifecycle investigation is complete. The real
TerminalView and retained-session provider showed no material degradation in
this DOM-renderer browser fixture after 100,000 output lines. Scrollback stayed
bounded, search/scroll/input checks passed, switches retained terminal instances,
and closing tabs released their large backing buffers. No product code change,
build, restart or deployment was justified by these results.

This is not proof of zero leaks. Post-close JavaScript heap and embedder heap
rose modestly across the three measured lifecycle rounds. The origin and longer
term behavior of that small growth were not established. Electron process RSS,
GPU/VRAM and an hours-long soak remain outside this completed finite test.

## Method

`desktop/tests/terminal-long-session.browser.mjs` imports the actual
TerminalSessionProvider, PersistentTerminalView and TerminalView. Only backend
RPC is simulated; pages follow the production 128 KiB output-page shape.
The browser uses DOM rendering with cursor blink disabled for consistent samples.
Output includes ANSI color changes and Chinese text. Four 25,000-line batches
produce 100,000 lines / 7,900,049 bytes. Each phase checks consumed byte offsets,
the last 20 rendered rows and the configured scrollback bound.

The fixture warms four tabs, then measures the large-output session and three
cycles of four tabs with 24 switches per cycle. It verifies switches do not create
extra terminals or reread output from zero. All 17 created instances, including
warmup and the long-output session, call dispose. Tracking uses WeakRef and deletes
entries on disposal, avoiding strong fixture references to disposed terminals.

Memory samples use CDP after two garbage collections separated by a short settle:
V8 used heap, backing storage (including TypedArray buffers), embedder heap, DOM
nodes and JS event listeners. Forced collection measures retained memory, not
normal-process peak memory or OS working set.

## Final measurements

| Output lines | Batch drain | JS heap bytes | Backing storage bytes | Retained rows |
| --- | ---: | ---: | ---: | ---: |
| 25,000 | 427 ms | 15,284,744 | 19,331,210 | 10,040 |
| 50,000 | 365 ms | 15,325,696 | 19,331,210 | 10,040 |
| 75,000 | 360 ms | 15,350,924 | 19,331,195 | 10,040 |
| 100,000 | 290 ms | 15,356,456 | 19,331,195 | 10,040 |

Retained rows are 10,000 scrollback plus a 40-row screen; this does not mean all
100,000 rows remain in frontend scrollback. All produced bytes were consumed;
backend log persistence is a separate existing implementation.
JS heap growth from 50k to 100k was 30,760 bytes and backing storage did not grow.
At 100k, JS heap was about 14.65 MiB and backing storage about 18.44 MiB; neither
figure should be presented as the application's total memory consumption.

After 100k lines, actual search selection completed in 33 ms. A mouse-wheel action
changed viewportY from 9917 to 9914 in 65 ms. Input reached the simulated backend
and its echo appeared in 22 ms. These include automation/fixture overhead and do
not promise native ConPTY timings. The three 24-switch rounds had median 83 ms
and maxima 100/100/96 ms; these also include two requestAnimationFrame waits.

| Post-close sample | JS heap bytes | Backing storage bytes | Embedder heap bytes | DOM nodes | Listeners |
| --- | ---: | ---: | ---: | ---: | ---: |
| Four-tab warm baseline | 10,774,528 | 4,341,501 | 1,337,096 | 42 | 890 |
| After long-output session | 11,055,000 | 4,341,766 | 1,179,192 | 39 | 461 |
| Lifecycle 1 | 11,733,344 | 4,341,808 | 1,812,672 | 42 | 890 |
| Lifecycle 2 | 11,917,772 | 4,341,808 | 1,937,064 | 42 | 890 |
| Lifecycle 3 | 12,103,236 | 4,341,808 | 2,009,480 | 42 | 890 |

Backing storage, node count and listener count plateau in the lifecycle samples.
The final JS heap remains 1,328,708 bytes (about 1.27 MiB) above the warm baseline,
and rises about 180 KiB per later round; the embedder heap also rises. These facts
remain open observations, not a proven leak diagnosis or a claim of zero growth.
The finite test's 12 MiB heap/backing-growth guards are coarse regression budgets,
not mathematical leak-detection criteria.

## Evidence quality and corrections

The first sample warmed one tab and compared it to four-tab topology. Its listener
guard failed at 461 versus 890, while all three measured cycles stayed at 890.
Rewarming the same four-tab topology resolved that baseline mismatch without
relaxing the guard or changing product code. The original result is retained as
`first-sample.json`; the first script revision was not separately archived.

The independent read-only `long_session_evidence_review` inspected the fixture,
provider and cleanup code plus the then-current results. It confirmed the
limited interpretation and identified a missing assertion that the wheel action
actually moves the viewport. That assertion was added and passed. A separate
final instrumentation completion added backing-storage samples after confirming
xterm stores its rows in Uint32Array. The primary agent ran the final test; the
reviewer did not independently rerun these last instrumentation additions.

## Delivery states and preservation

- Implementation: no production changes; reusable pressure test added.
- Development verification: final pressure test passed in 18.8 seconds.
- Independent review: evidence/method reviewed, with limits and corrections above.
- Technical acceptance: finite browser test complete; long-duration leak freedom
  and whole-process/GPU memory are not established.
- Integration/deployment: none required; current installed candidate retained.
- Online/user UAT: no live user terminal was used or restarted by this test.
  The earlier user-accepted startup improvement is unchanged.

Installed app SHA-256 remains
`c45e6efa7124c9933f745a647f41c09be848303e965573c4087e5e84367baf41`.
TerminalView's source hash still matches the accepted export-receipt implementation.
The isolated test browser and server were closed by the fixture's finally block.

Evidence directory: `artifacts/terminal-long-session-20261028/`:
`result.json`, `test.log`, `provenance.json`, `first-sample.json`,
`pre-scroll-assertion.json` and `js-heap-sample.json`.
