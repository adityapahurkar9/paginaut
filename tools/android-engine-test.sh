#!/usr/bin/env bash
# Checks the isolated engine (engine_link.rs) on a running emulator or phone with Paginaut open: documents
# keep working after the engine's process is killed, mid-call and mid-search; a document that stops the engine
# twice is refused; locked documents stay unlocked; covers, printing and repaired e-books write through handles.
#
# Usage: CDP_PORT=9334 bash tools/android-engine-test.sh
# Needs, in /sdcard/Books (the shelf's chosen folder): nse.pdf, locked.pdf (password "test"), businessplans.epub.
set -uo pipefail
cd "$(dirname "$0")/.."
adb="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}/platform-tools/adb"
export MSYS_NO_PATHCONV=1
fail=0
uri() { echo "content://com.android.externalstorage.documents/tree/primary%3ABooks/document/$(node -e "process.stdout.write(encodeURIComponent('primary:Books/'+process.argv[1]))" "$1")"; }
js() { node tools/cdp.mjs eval "$1" | tr -d '"'; }
engine_pid() { "$adb" shell pidof app.paginaut.reader:engine:app.paginaut.reader.EngineService | tr -d '\r'; }
# Stops the engine as a crash would (a debug build's engine_crash_test: killing another app's process needs root).
kill_engine() { js "T.I('engine_crash_test')" >/dev/null; }
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2', wanted '$3'"; fail=1; fi; }

# Helpers in the page: open by shelf URI, draw a page, run a search to the end.
js "window.T = {
  I: window.__TAURI_INTERNALS__.invoke,
  async open(u) { const p = await this.I('reopen', { uri: u }); return (await this.I('open_document', { path: p.path })).docId; },
  async draw(d, page) { const r = await fetch('http://paginaut.localhost/render?d=' + d + '&p=' + page + '&s=0.5&r=0&e=1&id=' + (1e6 + page) + '&q=0'); return r.status; },
  // Whether a page draws anything (a locked file read with the wrong keys draws blank, still with 200).
  async inked(d, page) { const r = await fetch('http://paginaut.localhost/render?d=' + d + '&p=' + page + '&s=0.5&r=0&e=1&id=' + (2e6 + page) + '&q=0');
    const b = new Uint8Array(await r.arrayBuffer()); let n = 0;
    for (let k = 8; k < b.length; k += 4) if (b[k] < 250 || b[k + 1] < 250 || b[k + 2] < 250) n++;
    return r.status === 200 && n > 0; },
  search(d, needle, sid) { return new Promise((done) => {
    let hits = 0, pages = new Set();
    const id = window.__TAURI_INTERNALS__.transformCallback((m) => { const e = m && m.message !== undefined ? m.message : m;
      if (e.kind === 'hits') { hits += e.hits.length; if (pages.has(e.page)) hits = -1e9; pages.add(e.page); }
      if (e.kind === 'done') done({ hits, complete: e.complete, error: e.error || '' }); });
    this.I('search', { doc: d, needle, opts: null, sid, onEvent: '__CHANNEL__:' + id }).catch((e) => done({ hits: 0, complete: false, error: String(e) }));
  }); },
}; 1" >/dev/null

# 1. A call after the engine stopped: it starts again and the document opens there again.
d=$(js "T.open('$(uri nse.pdf)')")
check "draw before" "$(js "T.draw($d, 0)")" 200
before=$(engine_pid)
kill_engine
check "draw after the engine was killed" "$(js "T.draw($d, 1)")" 200
after=$(engine_pid)
[ -n "$after" ] && [ "$after" != "$before" ] && echo "ok   new engine ($before -> $after)" || { echo "FAIL engine not restarted ($before -> $after)"; fail=1; }
check "text after restart" "$(js "T.I('page_text_stats', { doc: $d, page: 3 }).then(s => s.chars > 0)")" true

# 2. A search running as the engine is killed: started again, every page once.
full=$(js "T.search($d, 'Board', 501).then(r => r.hits)")
js "window.S = T.search($d, 'Board', 502); 1" >/dev/null
sleep 0.7
kill_engine
check "search across a restart" "$(js "S.then(r => r.complete + '/' + r.hits)")" "true/$full"

# 3. Killed twice within two minutes while working on the document: refused.
js "window.S = T.search($d, 'the', 503); 1" >/dev/null
sleep 0.7
kill_engine
sleep 0.7
kill_engine
check "refused after two stops" "$(js "S.then(r => r.error.includes('twice'))")" true
check "refused document draws no more" "$(js "T.draw($d, 2)")" 500
js "T.I('close_document', { doc: $d })" >/dev/null

# 4. An engine that stops answering (a file that makes it loop): let go by the watchdog after 45 s, and the
#    document works again on a new one.
h=$(js "T.open('$(uri nse.pdf)')")
before=$(engine_pid)
check "hung engine let go" "$(js "T.I('engine_crash_test', { how: 'hang' }).then(() => 'answered', e => String(e))")" "Paginaut's engine stopped."
check "draw after a hang" "$(js "T.draw($h, 4)")" 200
after=$(engine_pid)
[ -n "$after" ] && [ "$after" != "$before" ] && echo "ok   stuck engine gone ($before -> $after)" || { echo "FAIL stuck engine still there ($before -> $after)"; fail=1; }
# A document closed while the engine is frozen (answers nothing): the close itself is watched, and returns
# once the watchdog lets that engine go.
js "window.H = T.I('engine_crash_test', { how: 'freeze' }).catch(() => 0); 1" >/dev/null
sleep 1
check "close during a hang returns" "$(js "Promise.race([T.I('close_document', { doc: $h }).then(() => 'closed'), new Promise(r => setTimeout(() => r('still waiting'), 70000))])")" closed
js "H" >/dev/null

# A search or Read mode job that goes quiet: the watchdog starts a new engine (90 s without a word).
q=$(js "T.open('$(uri text-1000.pdf)')")
check "quiet stream noticed" "$(js "T.I('engine_crash_test', { how: 'silent', doc: $q }).then(r => r.newEngine && r.seconds < 110)")" true
check "draw after a quiet stream" "$(js "T.draw($q, 1)")" 200
js "T.I('close_document', { doc: $q })" >/dev/null

# 5. An engine asking for more temporary files than any reading needs gets no more than the cap.
check "temporary files capped" "$(js "T.I('engine_crash_test', { how: 'flood' }).then(r => r.got + ' ' + (r.error || ''))")" "512 The reading engine asked for too many temporary files."
check "a new engine starts with none" "$(js "T.I('engine_crash_test').catch(() => 0).then(() => T.I('engine_crash_test', { how: 'flood' })).then(r => r.got)")" 512

# 6. A locked document stays unlocked across a restart.
l=$(js "T.open('$(uri locked.pdf)')")
check "unlock" "$(js "T.I('unlock_document', { doc: $l, password: 'test' }).then(i => !!i && !i.needsPassword)")" true
check "unlocked document draws" "$(js "T.inked($l, 0)")" true
check "wrong password after unlocking" "$(js "T.I('unlock_document', { doc: $l, password: 'nope' }).then(i => i === null)")" true
check "still draws after a wrong password" "$(js "T.inked($l, 0)")" true
kill_engine
check "locked document after restart" "$(js "T.inked($l, 0)")" true
js "T.I('close_document', { doc: $l })" >/dev/null

# 7. Files written through handles: a cover, a print PDF, a repaired e-book.
check "cover" "$(js "T.I('reopen', { uri: '$(uri businessplans.epub)' }).then(p => T.I('book_cover', { path: p.path, fp: 'ab12' })).then(m => m.cover + '/' + m.pages)")" "true/206"
b=$(js "T.open('$(uri businessplans.epub)')")
bpath=$(js "T.I('reopen', { uri: '$(uri businessplans.epub)' }).then(p => p.path)")
cache=${bpath%%/cache/*}/cache
check "print PDF" "$(js "T.I('print', { doc: $b, path: '$bpath', title: 't', pdf: false }).then(() => 'ok', e => String(e))")" ok
sleep 2
"$adb" shell input keyevent KEYCODE_BACK >/dev/null
for kind in true false; do
  out="$cache/docs/test-repaired/book-$kind.$([ $kind = true ] && echo pdf || echo epub)"
  check "save repaired ($kind)" "$(js "T.I('save_repaired', { doc: $b, target: '$out', pdf: $kind }).then(() => 'ok', e => String(e))")" ok
  size=$("$adb" shell "run-as app.paginaut.reader stat -c %s '${out#/data/user/0/app.paginaut.reader/}'" | tr -d '\r')
  [ "${size:-0}" -gt 10000 ] && echo "ok   repaired file written ($size bytes)" || { echo "FAIL repaired file: '$size'"; fail=1; }
done
js "T.I('close_document', { doc: $b })" >/dev/null
"$adb" shell input keyevent KEYCODE_BACK >/dev/null

exit $fail
