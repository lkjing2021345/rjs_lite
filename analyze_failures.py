#!/usr/bin/env python3
"""Per-subdirectory pass/fail breakdown for the 2000-test language sample."""
import os, re, collections, subprocess, concurrent.futures as cf, tempfile

ROOT = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(ROOT, "target", "release", "rjs_lite.exe")
STA = os.path.join(ROOT, "test262", "harness", "sta.js")
ASSERT = os.path.join(ROOT, "test262", "harness", "assert.js")
STA_SRC = open(STA, encoding="utf-8").read()
ASSERT_SRC = open(ASSERT, encoding="utf-8").read()
TMPDIR = os.path.join(ROOT, "target", "t262tmp")
os.makedirs(TMPDIR, exist_ok=True)


def parse_frontmatter(src):
    m = re.search(r"/\*---\s*(.*?)\s*---\*/", src, re.S)
    if not m:
        return {}
    fm = {}
    cur_key = None
    for line in m.group(1).split("\n"):
        line = line.rstrip()
        if not line.strip():
            continue
        if line.startswith(" "):
            if cur_key:
                fm[cur_key] = fm.get(cur_key, "") + " " + line.strip()
            continue
        if ":" in line:
            k, v = line.split(":", 1)
            k, v = k.strip(), v.strip()
            cur_key = k
            if v:
                fm[k] = v
    return fm


def run_one(path):
    try:
        src = open(path, encoding="utf-8").read()
    except Exception:
        return ("read-error", path)
    fm = parse_frontmatter(src)
    flags = fm.get("flags", "")
    negative = fm.get("negative", "")
    only_strict = "onlyStrict" in flags
    base_code = STA_SRC + "\n" + ASSERT_SRC + "\n" + src
    modes = [True] if only_strict else [False, True]

    def run(strict):
        code = ('"use strict";\n' if strict else "") + base_code
        fd, tmp = tempfile.mkstemp(suffix=".js", dir=TMPDIR)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as fh:
                fh.write(code)
            return subprocess.run([BIN, tmp], capture_output=True, text=True, timeout=10)
        finally:
            try:
                os.unlink(tmp)
            except OSError:
                pass

    results = [run(s) for s in modes]
    is_negative = negative.strip().startswith("phase:")
    if is_negative:
        phase = "parse" if "phase: parse" in negative else "runtime"
        ok = False
        for r in results:
            if r.returncode != 0:
                err = (r.stderr + r.stdout).lower()
                if phase == "parse" and ("syntaxerror" in err or "parse" in err or "lexer" in err):
                    ok = True
                    break
                if phase == "runtime":
                    ok = True
                    break
        return ("pass" if ok else "negative-phase-mismatch", path)
    else:
        ok = all(r.returncode == 0 for r in results)
        return ("pass" if ok else "runtime-error", path)


def main():
    base = os.path.join(ROOT, "test262", "test", "language")
    files = []
    for dirpath, _, filenames in os.walk(base):
        for f in filenames:
            if f.endswith(".js") and not f.endswith("_FIXTURE.js"):
                files.append(os.path.join(dirpath, f))
    files.sort()
    files = files[:2000]

    counts = collections.Counter()
    subdir_total = collections.Counter()
    with cf.ThreadPoolExecutor(max_workers=32) as ex:
        for res, path in ex.map(run_one, files):
            rel = os.path.relpath(path, base)
            parts = rel.split(os.sep)
            sub = parts[0] if len(parts) > 1 else "(root)"
            subdir_total[sub] += 1
            counts[(sub, res)] += 1

    print(f"{'subdir':<24}{'total':>6}{'pass':>6}{'fail':>6}{'rate':>7}")
    for sub in sorted(subdir_total, key=lambda s: -subdir_total[s]):
        tot = subdir_total[sub]
        p = counts.get((sub, "pass"), 0)
        f = tot - p
        print(f"{sub:<24}{tot:>6}{p:>6}{f:>6}{p / tot * 100:>6.1f}%")
    print("TOTAL", sum(subdir_total.values()), "pass", sum(v for (s, r), v in counts.items() if r == "pass"))


if __name__ == "__main__":
    main()
