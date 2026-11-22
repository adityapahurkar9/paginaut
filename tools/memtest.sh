# Usage: bash tools/memtest.sh <cpuCanvas 0|1> <file> -> opens the file (a heavy scan), read-scrolls, prints memory
MODE=$1
FILE=$(cygpath -m "${2:?give a PDF to open, such as a heavy scan}")
powershell -NoProfile -File tools/dev.ps1 -NoBuild -Fresh > /dev/null
sleep 3
node tools/cdp.mjs eval "localStorage.setItem('paginaut.cpuCanvas','$MODE'); location.reload(); 1" > /dev/null
sleep 3
node tools/cdp.mjs eval "(async()=>{const o=await ($(cat tools/bench-open.js))('$FILE'); await new Promise(r=>setTimeout(r,500)); const s=await ($(cat tools/bench-scroll.js))(40,600); return {...o,...s}})()" | tr -d '\n '; echo
sleep 4
powershell -NoProfile -File tools/mem.ps1
