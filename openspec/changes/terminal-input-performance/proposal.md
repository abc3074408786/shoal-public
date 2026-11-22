# Reliable clipboard input and bounded performance evidence

Repair the Electron 44 clipboard API incompatibility for text and images. Preserve
existing clipboard payload types, ordering, session ownership and failure policy.
Await writes and read a single snapshot; support the existing older-Electron path.

Add a 512-entry memory-only timing recorder per terminal. Capture native input
categories, IME boundaries, queue wait, input acknowledgement, output receipt,
parser completion, render callback and a later browser frame opportunity. Do not
record keystrokes, command text, output text, clipboard contents, account data or
network addresses. Export JSON only via an explicit terminal menu action. A frame
opportunity must not be described as physical display completion.

Acceptance: helper tests, real Electron text/image clipboard checks (restore the
original clipboard), real TerminalView paste/ordering/IME/timing export regressions,
build and independent review. Install only the accepted renderer plus the narrow
clipboard IPC change, retaining installed runtime and user sessions. Real OS IME
and full Claude startup UAT remain explicitly separate from synthetic event tests.
