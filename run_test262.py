#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Optional test262 exploration runner for rjs_lite.

Prerequisites:
- Build `target/release/rjs_lite.exe` with `cargo build --release`
- Place an upstream test262 checkout at `./test262` (or pass `-d DIR`)

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

Subset filtering:
- --category DIR[,DIR...]    only run tests under the given top-level dir(s)
                             (test/language, test/built-ins, ...)
- --skip-category DIR[,DIR]  exclude given top-level dir(s)
- --list-categories          print available top-level categories and exit
- --limit N                  run at most N tests (smoke runs)

Failure categorization:
- Every failure is classified: timeout | parse-error | runtime-error |
  negative-phase-mismatch | negative-type-mismatch | negative-but-passed |
  async-not-complete | read-error
- The report (test262_report.json) records per-category counts, sample paths
  per category, and a per-category pass/fail breakdown.

Pass-rate tracking:
- test262_report.json (gitignored) records date, git commit, counts,
  pass rates, per-category failure breakdown, elapsed time.
- test262_result.json is kept for backward compatibility.
"""
import argparse, os, re, sys, subprocess, tempfile, concurrent.futures, threading, time, json, datetime

ROOT = os.path.dirname(os.path.abspath(__file__))
HARNESS = None
STA = None
ASSERT = None
TIMEOUT = 10  # seconds per run

CAT_TIMEOUT = "timeout"
CAT_PARSE = "parse-error"
CAT_RUNTIME = "runtime-error"
CAT_NEG_PHASE = "negative-phase-mismatch"
CAT_NEG_TYPE = "negative-type-mismatch"
CAT_NEG_PASSED = "negative-but-passed"
CAT_ASYNC = "async-not-complete"
CAT_READ = "read-error"
FAIL_CATEGORIES = [CAT_TIMEOUT, CAT_PARSE, CAT_RUNTIME, CAT_NEG_PHASE,
                   CAT_NEG_TYPE, CAT_NEG_PASSED, CAT_ASYNC, CAT_READ]

def load_harness(harness_dir):
    global HARNESS, STA, ASSERT
    HARNESS = harness_dir
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

def collect(test_dir, categories=None, skip_categories=None):
    files = []
    for dp, dn, fn in os.walk(test_dir):
        for f in fn:
            if not f.endswith(".js"):
                continue
            if f.endswith("_FIXTURE.js"):
                continue
            path = os.path.join(dp, f)
            rel = os.path.relpath(path, test_dir)
            top = rel.split(os.sep)[0]
            if categories and top not in categories:
                continue
            if skip_categories and top in skip_categories:
                continue
            files.append(path)
    return files

def list_categories(test_dir):
    cats = set()
    for dp, dn, fn in os.walk(test_dir):
        for f in fn:
            if f.endswith(".js") and not f.endswith("_FIXTURE.js"):
                rel = os.path.relpath(os.path.join(dp, f), test_dir)
                cats.add(rel.split(os.sep)[0])
    return sorted(cats)

def run_one(path, bin_path, timeout):
    """Return (result, category, detail).
    result is 'pass'|'fail'|'skip'; category is a failure category string
    (empty for pass/skip)."""
    try:
        with open(path, encoding="utf-8") as f:
            src = f.read()
    except OSError as e:
        return ("skip", CAT_READ, "read:" + str(e))
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
                p = subprocess.run([bin_path, tf.name], capture_output=True, text=True,
                                   timeout=timeout)
            except subprocess.TimeoutExpired:
                return ("fail", CAT_TIMEOUT, "timeout")
            failed = p.returncode != 0
            err = (p.stderr or "")
            phase = None
            err_type = None
            if failed:
                if err.startswith("SyntaxError:"):
                    phase = "parse"
                else:
                    phase = "runtime"
                m = re.match(r"(\w+Error):", err)
                if m:
                    err_type = m.group(1)

            if neg:
                if not failed:
                    return ("fail", CAT_NEG_PASSED, "negative-but-passed")
                if neg["phase"] and phase != neg["phase"]:
                    return ("fail", CAT_NEG_PHASE, "phase %s!=%s" % (phase, neg["phase"]))
                if neg["type"] and err_type and neg["type"] != err_type:
                    return ("fail", CAT_NEG_TYPE, "type %s!=%s" % (neg["type"], err_type))
                continue
            else:
                if failed:
                    cat = CAT_PARSE if phase == "parse" else CAT_RUNTIME
                    detail = err.strip().splitlines()[0] if err.strip() else "exit%d" % p.returncode
                    return ("fail", cat, detail)
                if is_async:
                    out = p.stdout or ""
                    if "Test262:AsyncTestComplete" not in out or "Test262:AsyncTestFailure" in out:
                        return ("fail", CAT_ASYNC, "async-not-complete")
                # this mode passes, continue
        finally:
            try: os.unlink(tf.name)
            except OSError: pass

    return ("pass", "", "")

def git_commit():
    try:
        p = subprocess.run(["git", "-C", ROOT, "rev-parse", "--short", "HEAD"],
                           capture_output=True, text=True, timeout=10)
        if p.returncode == 0:
            return p.stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        pass
    return None

def main():
    ap = argparse.ArgumentParser(description="test262 subset runner for rjs_lite")
    ap.add_argument("-d", "--test262-dir", default=os.path.join(ROOT, "test262"),
                    help="path to test262 checkout (default: ./test262)")
    ap.add_argument("-b", "--bin", default=os.path.join(ROOT, "target", "release", "rjs_lite.exe"),
                    help="path to rjs_lite binary (default: target/release/rjs_lite.exe)")
    ap.add_argument("--category", default=None,
                    help="comma-separated top-level dirs to include (language, built-ins, ...)")
    ap.add_argument("--skip-category", default=None,
                    help="comma-separated top-level dirs to exclude")
    ap.add_argument("--list-categories", action="store_true",
                    help="list available top-level categories and exit")
    ap.add_argument("--limit", type=int, default=None,
                    help="run at most N tests (smoke runs)")
    ap.add_argument("--workers", type=int, default=None,
                    help="worker count (default: min(32, cpu*4))")
    ap.add_argument("--timeout", type=float, default=TIMEOUT,
                    help="per-run timeout seconds (default: 10)")
    ap.add_argument("--out", default=os.path.join(ROOT, "test262_report.json"),
                    help="report output path (default: test262_report.json)")
    ap.add_argument("--quiet", action="store_true", help="suppress progress prints")
    args = ap.parse_args()

    test_dir = os.path.join(args.test262_dir, "test")
    harness_dir = os.path.join(args.test262_dir, "harness")
    if not os.path.isdir(test_dir):
        print("test262 test dir not found: %s" % test_dir, file=sys.stderr)
        print("place an upstream test262 checkout at %s (or pass -d DIR)" % args.test262_dir,
              file=sys.stderr)
        sys.exit(1)
    if not os.path.isfile(args.bin):
        print("engine binary not found: %s" % args.bin, file=sys.stderr)
        print("build it with: cargo build --release", file=sys.stderr)
        sys.exit(1)
    load_harness(harness_dir)

    categories = [c.strip() for c in args.category.split(",")] if args.category else None
    skip_categories = [c.strip() for c in args.skip_category.split(",")] if args.skip_category else None

    if args.list_categories:
        for c in list_categories(test_dir):
            print(c)
        return

    files = collect(test_dir, categories, skip_categories)
    if args.limit is not None:
        files = files[:args.limit]
    total = len(files)
    if not args.quiet:
        print("total tests (non-fixture):", total, flush=True)
        if categories:
            print("included categories:", ", ".join(categories), flush=True)
        if skip_categories:
            print("skipped categories:", ", ".join(skip_categories), flush=True)

    counts = {"pass": 0, "fail": 0, "skip": 0}
    cat_counts = {c: 0 for c in FAIL_CATEGORIES}
    cat_samples = {c: [] for c in FAIL_CATEGORIES}
    cat_stats = {}  # top-level category -> {"pass": n, "fail": n}
    lock = threading.Lock()
    done = [0]
    start = time.time()

    def work(path):
        res, cat, detail = run_one(path, args.bin, args.timeout)
        top = os.path.relpath(path, test_dir).split(os.sep)[0]
        with lock:
            counts[res] += 1
            done[0] += 1
            s = cat_stats.setdefault(top, {"pass": 0, "fail": 0})
            if res in ("pass", "fail"):
                s[res] += 1
            if res == "fail":
                cat_counts[cat] += 1
                if len(cat_samples[cat]) < 10:
                    cat_samples[cat].append(os.path.relpath(path, ROOT))
            if not args.quiet and done[0] % 2000 == 0:
                el = time.time() - start
                print("  %d/%d  pass=%d fail=%d skip=%d  %.0fs" %
                      (done[0], total, counts["pass"], counts["fail"], counts["skip"], el),
                      flush=True)

    workers = args.workers or min(32, (os.cpu_count() or 4) * 4)
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as ex:
        list(ex.map(work, files))

    el = time.time() - start
    denom = counts["pass"] + counts["fail"]
    rate = (counts["pass"] / denom * 100) if denom else 0.0
    rate_total = (counts["pass"] / total * 100) if total else 0.0

    print("\n=== RESULTS ===", flush=True)
    print("total :", total)
    print("pass  :", counts["pass"])
    print("fail  :", counts["fail"])
    print("skip  :", counts["skip"])
    print("pass rate (pass / (pass+fail)) : %.4f%%" % rate)
    print("pass rate (pass / total)       : %.4f%%" % rate_total)
    print("elapsed: %.0fs  workers=%d" % (el, workers))

    print("\nfailure categories:")
    for c in FAIL_CATEGORIES:
        if cat_counts[c]:
            print("  %-26s %d" % (c, cat_counts[c]))
    print("\nper-category breakdown:")
    for top in sorted(cat_stats):
        s = cat_stats[top]
        d = s["pass"] + s["fail"]
        r = (s["pass"] / d * 100) if d else 0.0
        print("  %-24s pass=%-6d fail=%-6d %.4f%%" % (top, s["pass"], s["fail"], r))

    print("\nsample failures:")
    for c in FAIL_CATEGORIES:
        if cat_samples[c]:
            print("  [%s]" % c)
            for rel in cat_samples[c][:5]:
                print("    ", rel)

    report = {
        "date": datetime.date.today().isoformat(),
        "commit": git_commit(),
        "total": total,
        "pass": counts["pass"],
        "fail": counts["fail"],
        "skip": counts["skip"],
        "pass_rate": round(rate, 4),
        "pass_rate_total": round(rate_total, 4),
        "failure_categories": {c: cat_counts[c] for c in FAIL_CATEGORIES},
        "top_failed_categories": {
            c: cat_samples[c] for c in FAIL_CATEGORIES if cat_samples[c]
        },
        "category_breakdown": {t: dict(s) for t, s in sorted(cat_stats.items())},
        "elapsed_sec": round(el, 1),
        "workers": workers,
        "filters": {
            "include": categories,
            "exclude": skip_categories,
            "limit": args.limit,
        },
    }
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)
    # backward-compatible summary
    with open(os.path.join(ROOT, "test262_result.json"), "w", encoding="utf-8") as f:
        json.dump({"total": total, **counts, "rate_pp": rate}, f, indent=2)
    print("\nreport written to:", args.out, flush=True)

if __name__ == "__main__":
    main()
