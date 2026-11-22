# Unmodified terminal dependencies

The application uses the official npm distributions of xterm 6.1.0-beta.303,
addon-webgl 0.20.0-beta.299, fit 0.12.0-beta.300, search 0.17.0-beta.300,
web-links 0.13.0-beta.300 and unicode-graphemes 0.4.0. Versions are pinned in
package.json and integrity hashes are recorded in package-lock.json. There is
no terminal postinstall patch step.

As of the independent-terminal change, the previously adapted Unicode,
selection, wheel-distance, wheel-dispatch and IME-anchor modules have been
replaced. Unicode is supplied by the official xterm addon; application code owns
selection margin removal, bounded linear wheel distance, painted-caret detection
and IME transactions. The latter runs before xterm's DOM handlers and sends
confirmed text through the public Terminal.input API. It does not modify the
terminal engine. The old WebGL and xterm patch files have been removed.

The previous Orca MIT notice remains at ../ORCA-LICENSE.txt for historical
versions. Its presence does not mean the new helpers are imported from Orca.
Upstream xterm/addon and font licenses still apply. This change is a bounded
source replacement, not a clean-room or whole-repository legal audit.

See ../../openspec/changes/independent-terminal for behavior, verification and
remaining native UAT boundaries. The existing persistent host, recorded geometry,
input provenance and session replay are unchanged by this replacement.
