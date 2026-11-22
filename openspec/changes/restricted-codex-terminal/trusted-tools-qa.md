# Restricted terminal trusted tool paths

The real terminal run found that `rg` was installed at
`/usr/lib/chatgpt/resources/rg`, while Restricted rebuilt `PATH` using only
`/usr/local/bin:/usr/bin:/bin`. The system binary was already inside a read-only
system rule; the missing command was a discovery problem.

`restricted_tools.rs` now discovers only four named tools (`rg`, `node`, `git`,
`python3`) at fixed system/package paths. Each selected executable and all of its
canonical parent directories must be root-owned and not group/other writable.
The process never imports the host `PATH`, user package directories, or the host
HOME to find tools.

The launcher creates `<private-home>.velella-tools-v1` beside the tool HOME and
prepends it to the reconstructed system `PATH`. It contains only validated
symlinks and receives a separate read/execute-only Landlock rule. The source
executables remain covered by the existing system rules. Any overlap with a
writable root, unexpected entry, substituted link, directory symlink, or unsafe
ownership/permissions fails before executing a tool. Repeated calls reuse the
same validated directory. Terminal session cleanup removes it with the owning
session directory.

Node on this machine also attempted to read `/etc/ssl/openssl.cnf` on startup.
The rebuilt environment now fixes `OPENSSL_CONF=/dev/null`, retaining default
OpenSSL providers without importing host configuration. The existing certificate
read rules are unchanged. Caller-provided `OPENSSL_CONF` is ignored.

The kernel restrictions remain the same: work and private HOME are writable;
tool aliases and system executables are not writable; outside authentication
files cannot be opened; addressable IPv4, IPv6 and Unix sockets are denied.
This implementation is for the existing Linux Restricted backend. It does not
install missing programs or provide arbitrary user-managed runtimes.

## Verification

Run the independent, model-free acceptance probe:

```sh
python3 scripts/verify-restricted-tools.py \
  --runtime target/debug/velella-runtime \
  --output artifacts/restricted-tools-20261020-v3
```

It uses a disposable work directory, HOME and synthetic authentication fixture.
It does not read real credentials or invoke a provider. Its final JSON records
source/runtime hashes and explicit checks for:

- Actual `rg` search plus Node, Git and Python execution inside `restricted-tool`.
- Initial preparation and reuse of the exact private tool directory.
- Rejected writes, deletion, chmod and replacement of tools.
- Denied authentication open and all three addressable socket families.
- Ignored hostile host PATH, credentials, loader, proxy and OpenSSL environment.
- Bare-name `rg` resolution through the `restricted-exec` entry point.
- A host-substituted alias being rejected before the payload can execute.
- Removal of every temporary fixture.

Rust tests cover trusted-source selection, exact/idempotent aliases, writable
root overlap, a symlinked directory, poisoned entries and ignored host OpenSSL
configuration. The initial five targeted tests passed. The v1/v2 acceptance
receipts preserve the discovered Node/OpenSSL failure.

The final rebuilt runtime passed **64/64** kernel acceptance checks in
`artifacts/restricted-tools-20261020-v3/result.json`, including the empty OpenSSL
configuration override and actual Node execution. The temporary fixture tree was
removed. Runtime SHA-256:
`e46aed22b8a6af52a29ece46abe2ab3cc3e7913d8d3fee5d51aeb9173555334a`.
This receipt is a functional/security regression check, not a performance
benchmark; no real model request was made.
