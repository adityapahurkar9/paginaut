# Code-signs one file (Tauri's bundle.windows.signCommand runs this for the app and the installer).
# Does nothing until a certificate is set up, so unsigned builds keep working. Set ONE of these, e.g. in your
# user environment variables (never in the repository):
#   PAGINAUT_SIGN_THUMBPRINT   SHA-1 thumbprint of a code-signing certificate in your Windows certificate store
#                              (a CA certificate on a USB token or in a cloud HSM that shows up there).
#   PAGINAUT_SIGN_DLIB, PAGINAUT_SIGN_METADATA
#                              Azure Trusted Signing: the path to Azure.CodeSigning.Dlib.dll and to its
#                              metadata.json (endpoint, account, certificate profile).
# PAGINAUT_SIGN_TIMESTAMP overrides the timestamp server.
param([Parameter(Mandatory)] [string]$File)

$thumb = $env:PAGINAUT_SIGN_THUMBPRINT
$dlib = $env:PAGINAUT_SIGN_DLIB
if (-not $thumb -and -not $dlib) {
  Write-Host "sign.ps1: no certificate set up; $([IO.Path]::GetFileName($File)) left unsigned."
  exit 0
}

# signtool from the newest Windows SDK.
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
  Sort-Object { [version]($_.Directory.Parent.Name) } -Descending | Select-Object -First 1
if (-not $signtool) { Write-Error "signtool.exe not found: install the Windows SDK's signing tools."; exit 1 }

if ($dlib) {
  $ts = if ($env:PAGINAUT_SIGN_TIMESTAMP) { $env:PAGINAUT_SIGN_TIMESTAMP } else { "http://timestamp.acs.microsoft.com" }
  & $signtool.FullName sign /v /fd SHA256 /tr $ts /td SHA256 /dlib $dlib /dmdf $env:PAGINAUT_SIGN_METADATA $File
} else {
  $ts = if ($env:PAGINAUT_SIGN_TIMESTAMP) { $env:PAGINAUT_SIGN_TIMESTAMP } else { "http://timestamp.digicert.com" }
  & $signtool.FullName sign /v /fd SHA256 /tr $ts /td SHA256 /sha1 $thumb $File
}
exit $LASTEXITCODE
