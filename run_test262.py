#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Optional test262 exploration runner for rjs_lite.

Prerequisites:
- Build `target/release/rjs_lite.exe` with `cargo build --release`
- Place an upstream test262 checkout at `./test262`

Methodology (honest, per test262/INTERPRETING.md as far as the engine allows):
- Walk test262/test/**/*.js , skip *_FIXTURE.js
- Skip intl402 (engine has no ECMA-402)  -> counted as "skipped"
- Parse YAML-ish frontmatter between /*--- and ---*/ (flags, negative, includes)
- Non-raw tests: prepend harness sta.js + assert.js + any `includes`
- Strict/non-strict: run both modes unless flags say otherwise; ALL required modes must pass
- Positive test passes  : process exit code == 0
        async test passes: stdout contains 'Test262:AsyncTestComplete' and no 'Test262:AsyncTestFailure'
- Negative test passes  : process failed AND failure phase matches (parse|runtime)
        LIMITATION: the runner does not verify the requested JS Error `type`
        (e.g. SyntaxError vs TypeError). Phase is matched by
        mapping our lexer/parser errors -> parse, runtime errors -> runtime.
- module tests: engine has no module system; run as plain script (will essentially fail)
"""
import os, re, sys, subprocess, tempfile, concurrent.futures, threading, time, json

ROOT = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(ROOT, "target", "release", "rjs_lite.exe")
TEST_DIR = os.path.join(ROOT, "test262", "test")
HARNESS = os.path.join(ROOT, "test262", "harness")
TIMEOUT = 10  # seconds per run

with open(os.path.join(HARNESS, "sta.js"), encoding="utf-8") as f:
    STA = f.read()
with open(os.path.join(HARNESS, "assert.js"), encoding="utf-8") as f:
    ASSERT = f.read()

_inc_cache = {}
def load_include(name):
    if name not in _inc_cache:
        try:
            with open(os.path.join(HARNESS, name), encoding="utf-8") as f:
                _inc_cache[name] = f.read()
        except OSError:
            _inc_cache[name] = ""
    return _inc_cache[name]

FM = re.compile(r"/\*---(.*?)---\*/", re.S)

def parse_meta(src):
    m = FM.search(src)
    meta = {"flags": [], "negative": None, "includes": []}
    if not m:
        return meta
    block = m.group(1)
    fm = re.search(r"flags:\s*\[(.*?)\]", block)
    if fm:
        meta["flags"] = [x.strip() for x in fm.group(1).split(",") if x.strip()]
    im = re.search(r"includes:\s*\[(.*?)\]", block, re.S)
    if im:
        meta["includes"] = [x.strip() for x in im.group(1).split(",") if x.strip()]
    nm = re.search(r"negative:\s*\n((?:\s+\w+:.*\n?)+)", block)
    if nm:
        nb = nm.group(1)
        phase = re.search(r"phase:\s*(\w+)", nb)
        typ = re.search(r"type:\s*(\w+)", nb)
        meta["negative"] = {
            "phase": phase.group(1) if phase else None,
            "type": typ.group(1) if typ else None,
        }
    return meta

def collect():
    files = []
    for dp, dn, fn in os.walk(TEST_DIR):
        for f in fn:
            if not f.endswith(".js"):
                continue
            if f.endswith("_FIXTURE.js"):
                continue
            files.append(os.path.join(dp, f))
    return files

def run_one(path):
    """Return ('pass'|'fail'|'skip', detail)."""
    try:
        with open(path, encoding="utf-8") as f:
            src = f.read()
    except OSError as e:
        return ("skip", "read:" + str(e))
    meta = parse_meta(src)
    flags = meta["flags"]
    neg = meta["negative"]

    if "module" in flags:
        modes = ["module"]
    elif "raw" in flags:
        modes = ["raw"]
    elif "onlyStrict" in flags:
        modes = ["strict"]
    elif "noStrict" in flags:
        modes = ["nonstrict"]
    else:
        modes = ["nonstrict", "strict"]

    is_async = "async" in flags

    for mode in modes:
        if mode == "raw":
            code = src
        else:
            parts = [STA, ASSERT]
            if is_async:
                parts.append(load_include("doneprintHandle.js"))
            for inc in meta["includes"]:
                parts.append(load_include(inc))
            body = src
            if mode == "strict":
                body = '"use strict";\n' + body
            parts.append(body)
            code = "\n".join(parts)

        tf = tempfile.NamedTemporaryFile("w", suffix=".js", delete=False, encoding="utf-8")
        try:
            tf.write(code)
            tf.close()
            try:
                p = subprocess.run([BIN, tf.name], capture_output=True, text=True,
                                   timeout=TIMEOUT)
            except subprocess.TimeoutExpired:
                return ("fail", "timeout")
            failed = p.returncode != 0
            err = (p.stderr or "")
            phase = None
            if failed:
                if err.startswith("lex error") or err.startswith("parse error"):
                    phase = "parse"
                else:
                    phase = "runtime"

            if neg:
                # expect failure at given phase
                if not failed:
                    return ("fail", "negative-but-passed")
                if neg["phase"] and phase != neg["phase"]:
                    return ("fail", "phase %s!=%s" % (phase, neg["phase"]))
                # phase ok (type not checked) -> this mode passes, continue
                continue
            else:
                if failed:
                    return ("fail", err.strip().splitlines()[0] if err.strip() else "exit%d" % p.returncode)
                if is_async:
                    out = p.stdout or ""
                    if "Test262:AsyncTestComplete" not in out or "Test262:AsyncTestFailure" in out:
                        return ("fail", "async-not-complete")
                # this mode passes, continue
        finally:
            try: os.unlink(tf.name)
            except OSError: pass

    return ("pass", "")

def main():
    files = collect()
    total = len(files)
    print("total tests (non-fixture):", total, flush=True)
    counts = {"pass": 0, "fail": 0, "skip": 0}
    lock = threading.Lock()
    done = [0]
    fail_samples = []
    start = time.time()

    def work(path):
        res, detail = run_one(path)
        with lock:
            counts[res] += 1
            done[0] += 1
            if res == "fail" and len(fail_samples) < 40:
                fail_samples.append((os.path.relpath(path, ROOT), detail))
            if done[0] % 2000 == 0:
                el = time.time() - start
                print("  %d/%d  pass=%d fail=%d skip=%d  %.0fs" %
                      (done[0], total, counts["pass"], counts["fail"], counts["skip"], el),
                      flush=True)

    workers = min(32, (os.cpu_count() or 4) * 4)
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as ex:
        list(ex.map(work, files))

    el = time.time() - start
    print("\n=== RESULTS ===", flush=True)
    print("total :", total)
    print("pass  :", counts["pass"])
    print("fail  :", counts["fail"])
    print("skip  :", counts["skip"])
    denom = counts["pass"] + counts["fail"]
    rate = (counts["pass"] / denom * 100) if denom else 0.0
    print("pass rate (pass / (pass+fail)) : %.4f%%" % rate)
    print("pass rate (pass / total)       : %.4f%%" % (counts["pass"] / total * 100))
    print("elapsed: %.0fs  workers=%d" % (el, workers))
    print("\nsample failures:")
    for rel, d in fail_samples[:40]:
        print("  ", rel, "->", d)
    with open(os.path.join(ROOT, "test262_result.json"), "w") as f:
        json.dump({"total": total, **counts, "rate_pp": rate}, f, indent=2)

if __name__ == "__main__":
    main()
