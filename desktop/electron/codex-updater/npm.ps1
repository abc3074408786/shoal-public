# Velella Codex update guard v1
if ($MyInvocation.ExpectingInput) {
  $input | & node (Join-Path $PSScriptRoot 'codex-npm.cjs') @args
} else {
  & node (Join-Path $PSScriptRoot 'codex-npm.cjs') @args
}
exit $LASTEXITCODE
