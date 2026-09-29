#!/usr/bin/env bash
# Consecutive-search benchmark on one file, for each private-copy release policy.
#   bash tools/search-bench.sh "<file>" [policies...]   (policies: keep now idle)
FILE="$1"; shift
POLICIES="${*:-keep now idle}"
EXE="E:\\Projects\\PDF-Reader-Editor\\src-tauri\\target\\release\\folio.exe"
TERMS=(Company Offer SEBI risk financial Company)

for P in $POLICIES; do
  echo "=== policy: $P"
  powershell -NoProfile -Command "Get-Process folio -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 300; Remove-Item -Recurse -Force \"\$env:LOCALAPPDATA\\app.folio.reader\\EBWebView\\Default\\Local Storage\" -ErrorAction SilentlyContinue; \$env:FOLIO_PRIVATE_POLICY='$P'; \$env:FOLIO_SEARCH_THREADS='${THREADS:-8}'; \$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9333'; Start-Process '$EXE'"
  sleep 4
  node tools/cdp.mjs eval "($(cat tools/bench-open.js))('$FILE')" > /dev/null
  sleep 3
  printf "%-22s %8s %10s %10s %12s\n" "step" "time" "private" "workset" "status bar"
  read PRIV WS < <(powershell -NoProfile -File tools/memnow.ps1)
  SB=$(node tools/cdp.mjs eval "window.__TAURI_INTERNALS__.invoke('system_info').then(s=>Math.round(s.processMemory/1048576))" | tr -d '"\n ')
  printf "%-22s %8s %8s MB %8s MB %10s MB\n" "opened" "-" "$PRIV" "$WS" "$SB"
  i=0
  for T in "${TERMS[@]}"; do
    i=$((i+1))
    MS=$(node tools/cdp.mjs eval "(async()=>{const f=window.folio; const tab=f.activeTab.value; const t0=performance.now(); f.actions.runSearch(tab,'$T'); await new Promise(r=>setTimeout(r,170)); while(!tab.search.value?.done || tab.search.value.query!=='$T') await new Promise(r=>setTimeout(r,5)); return Math.round(performance.now()-t0-160)})()" | tr -d '"\n ')
    sleep 2
    read PRIV WS < <(powershell -NoProfile -File tools/memnow.ps1)
    SB=$(node tools/cdp.mjs eval "window.__TAURI_INTERNALS__.invoke('system_info').then(s=>Math.round(s.processMemory/1048576))" | tr -d '"\n ')
    printf "%-22s %6s ms %8s MB %8s MB %10s MB\n" "search $i: $T" "$MS" "$PRIV" "$WS" "$SB"
  done
  sleep 33
  read PRIV WS < <(powershell -NoProfile -File tools/memnow.ps1)
  SB=$(node tools/cdp.mjs eval "window.__TAURI_INTERNALS__.invoke('system_info').then(s=>Math.round(s.processMemory/1048576))" | tr -d '"\n ')
  printf "%-22s %8s %8s MB %8s MB %10s MB\n" "35 s after last search" "-" "$PRIV" "$WS" "$SB"
done
powershell -NoProfile -Command "Get-Process folio -ErrorAction SilentlyContinue | Stop-Process -Force"
