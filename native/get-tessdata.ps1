# Fetches the OCR models built into Paginaut (Tesseract's fast models for English,
# Hindi and Marathi) into native/tessdata, checked against pinned SHA-256 hashes.
# They're bundled into the installer (tauri.conf.json "resources"); other languages
# and the "best" models are downloaded by the app when asked for.
# Source: https://github.com/tesseract-ocr/tessdata_fast, tag 4.1.0.
$ErrorActionPreference = "Stop"
$tag = "4.1.0"
$models = [ordered]@{
  eng = "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
  hin = "4c73ffc59d497c186b19d1e90f5d721d678ea6b2e277b719bee4e2af12271825"
  mar = "0ba3f2d116972e72fe9e176bc84c38e81dfb6670f4ed1f7f6c8e16a27da7cb61"
}
$dir = Join-Path $PSScriptRoot "tessdata"
New-Item -ItemType Directory -Force $dir | Out-Null
foreach ($lang in $models.Keys) {
  $file = Join-Path $dir "$lang.traineddata"
  if ((Test-Path $file) -and (Get-FileHash $file -Algorithm SHA256).Hash -eq $models[$lang]) {
    Write-Host "$lang ok"
    continue
  }
  $url = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/$tag/$lang.traineddata"
  Write-Host "downloading $url"
  Invoke-WebRequest $url -OutFile "$file.part" -UseBasicParsing
  if ((Get-FileHash "$file.part" -Algorithm SHA256).Hash -ne $models[$lang]) {
    Remove-Item "$file.part"
    throw "$lang.traineddata doesn't match its pinned hash"
  }
  Move-Item -Force "$file.part" $file
  Write-Host "$lang ok"
}
