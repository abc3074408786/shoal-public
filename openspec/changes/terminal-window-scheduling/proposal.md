# Keep terminal scheduling responsive when the desktop is hidden

The installed main window inherits Chromium background timer throttling. Its
terminal output pump uses short, serial timer steps; minimizing or hiding the
window can stretch them to roughly one second each. In an isolated native Electron
44.9.2 check, twelve 8 ms steps took 11,616 ms with the default enabled and 139 ms
with throttling disabled. This is a reproducible delay source, not proof that every
reported foreground slowdown has the same cause.

Set backgroundThrottling=false on the terminal desktop BrowserWindow. Retain
context isolation, sandboxing, web security, runtime, user data and native CLI
configuration. The application still controls its own polling/backoff. Expect
some additional background work compared with Chromium's hidden-window limit.

Validate a native hidden-window scheduling regression and JavaScript syntax;
apply only this option to the installed Electron main. Back up app.asar before
replacement, preserve live session hosts and history, and verify the installed
option and connected desktop after reopening. Real-user startup UAT remains a
separate confirmation of whether this covers the reported >10 second experience.
