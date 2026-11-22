# Native export receipt acceptance — 2026-10-27

## User-visible result

The installed terminal now writes performance JSON to the OS Downloads directory
and displays its full path only after the write completes. A write failure is
displayed explicitly. This export repair is implemented, independently reviewed,
installed and verified through the native UI and an actual readable file.

The earlier browser-download route did not produce an observable installed-file
receipt. Its exact failure cause was not established; UI automation also showed
focus/coordinate inconsistencies. This change removes that ambiguity through an
explicit native write and visible result, without asserting an unproven root cause.

## Implementation and validation

- Renderer sends only the snapshot to an allowlisted, trusted-main-frame IPC.
- Main process selects the OS Downloads directory and a unique exclusive-write
  filename. Renderer paths are not accepted. Existing exports are not overwritten.
- The writer reconstructs only allowed metadata and rejects invalid/oversized
  records. Commands, terminal output and clipboard bodies are not saved.
- Non-Electron browser download behavior remains available, with wording that
  accurately says a download was requested rather than confirming file completion.
- Six unit tests passed: four native export tests plus two recorder tests.
- Browser integration passed for the existing export, native success path with
  a real filesystem write, and native write failure with no stale success message.
- Build/type checks and scoped diff checks passed; the existing bundle-size
  warning remains.
- Independent read-only reviewer `export_receipt_review` found no blocking defect
  and independently ran the four export tests successfully.

## Integration, installation and online verification

The package was built from the installed archive with narrow main/preload/security
patches, the new helper and rebuilt renderer. Ten other non-renderer files were
verified byte-identical. Installed runtime, clipboard handling and the scheduling
repair were preserved. A first packaging attempt failed on Windows ASAR path
separators before installation; the corrected attempt passed in a fresh stage.

- Installed app SHA-256:
  `dcade3aa6efc54a09dd7d65854ba9ee434ed154e8671c3e74715c876829f5dda`
- Runtime SHA-256 (unchanged):
  `01c24d59951e96c204b41e02a58059bb0b18ca1157fc31910cb557f0966a123b`
- Deployment receipt: `artifacts/terminal-export-receipt/installation.json`
- Rollback backup: `artifacts/terminal-export-receipt/backup/`
- Daemon PID 31828 and protected process identities/history prefixes were preserved.
- Receipt status: `installed_verified`, scoped to this export change.

The native toolbar export produced the visible path below; the same file was read
back and matched the owned verification session. It contains 58 metadata entries
with zero dropped entries:

`C:/Users/developer/Downloads/velella-terminal-performance-1793111778213-62dbfe7f-690b-48fe-8b28-890907b29bc3.json`

Evidence copy: `artifacts/terminal-export-receipt/installed-performance-export.json`.
Native receipt: `artifacts/terminal-export-receipt/native-acceptance.json`.
Only the owned test process `01a17dc8-f31c-75ac-9195-bfa31249681c` was stopped;
original user CLI sessions were not sent test prompts or stopped.

## Startup measurement and remaining limits

In this one launch, `claude` was pasted into the owned PowerShell session and
submitted with native Return. No trust approval or model prompt was sent.

| Measurement | Result |
| --- | --- |
| Input acknowledgement | 19 ms |
| Main welcome-screen bytes received after Return | 1,398.2 ms |
| Render callback after those bytes arrived | 21.2 ms |
| Later frame opportunity after Return | 1,445.8 ms |
| Complete native screen observed by screenshot | 8,302 ms upper bound |

The byte range 460–4196 was checked against the owned session and contained the
Claude welcome screen. ANSI cursor motion inserts visual spaces without literal
space characters; the check normalizes those controls. Raw session contents were
not added to the exported performance JSON.

This is not a 1.45-second physical-display guarantee. Screenshot sampling cannot
identify the exact first visible instant; the command was pasted rather than
typed letter by letter. Repeated keyboard-startup/user-perceived UAT remains
unverified. The scoped first-round functionality and export acceptance are closed;
the broader claim that all startup slowness is eliminated has not been established.
