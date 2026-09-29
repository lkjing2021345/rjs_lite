#!/usr/bin/env python3
import subprocess, tempfile, os, sys
ROOT = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(ROOT, "target", "release", "rjs_lite.exe")
HARNESS = os.path.join(ROOT, "test262", "harness")
STA = open(os.path.join(HARNESS, "sta.js"), encoding="utf-8").read()
ASSERT = open(os.path.join(HARNESS, "assert.js"), encoding="utf-8").read()
inc = {}
def load_include(n):
    if n not in inc:
        try: inc[n] = open(os.path.join(HARNESS, n), encoding="utf-8").read()
        except OSError: inc[n] = ""
    return inc[n]
import re
FM = re.compile(r"/\*---(.*?)---\*/", re.S)
def run(path):
    src = open(path, encoding="utf-8").read()
    m = FM.search(src)
    fl = []; incs = []
    if m:
        b = m.group(1)
        fm = re.search(r"flags:\s*\[(.*?)\]", b)
        if fm: fl = [x.strip() for x in fm.group(1).split(",") if x.strip()]
        im = re.search(r"includes:\s*\[(.*?)\]", b, re.S)
        if im: incs = [x.strip() for x in im.group(1).split(",") if x.strip()]
    if "raw" in fl:
        code = src
    else:
        parts = [STA, ASSERT]
        if "async" in fl: parts.append(load_include("doneprintHandle.js"))
        for i in incs: parts.append(load_include(i))
        parts.append(src)
        code = "\n".join(parts)
    tf = tempfile.NamedTemporaryFile("w", suffix=".js", delete=False, encoding="utf-8")
    tf.write(code); tf.close()
    try:
        p = subprocess.run([BIN, tf.name], capture_output=True, text=True, timeout=10)
        print("rc:", p.returncode)
        print("stderr:", (p.stderr or "")[:500])
        print("stdout:", (p.stdout or "")[:300])
    finally:
        os.unlink(tf.name)
if __name__ == "__main__":
    run(sys.argv[1])
