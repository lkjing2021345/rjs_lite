import os, re, subprocess, tempfile, concurrent.futures as cf, collections
ROOT = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(ROOT, "target", "release", "rjs_lite.exe")
STA = open(os.path.join(ROOT, "test262", "harness", "sta.js"), encoding="utf-8").read()
ASSERT = open(os.path.join(ROOT, "test262", "harness", "assert.js"), encoding="utf-8").read()
TMPDIR = os.path.join(ROOT, "target", "t262tmp")
os.makedirs(TMPDIR, exist_ok=True)


def parse_frontmatter(src):
    m = re.search(r"/\*---\s*(.*?)\s*---\*/", src, re.S)
    if not m:
        return {}
    fm = {}
    cur = None
    for line in m.group(1).split("\n"):
        line = line.rstrip()
        if not line.strip():
            continue
        if line.startswith(" "):
            if cur:
                fm[cur] = fm.get(cur, "") + " " + line.strip()
            continue
        if ":" in line:
            k, v = line.split(":", 1)
            k, v = k.strip(), v.strip()
            cur = k
            if v:
                fm[k] = v
    return fm


def run_one(path):
    try:
        src = open(path, encoding="utf-8").read()
    except Exception:
        return ("read-error", path, "")
    fm = parse_frontmatter(src)
    flags = fm.get("flags", "")
    negative = fm.get("negative", "")
    only_strict = "onlyStrict" in flags
    base_code = STA + "\n" + ASSERT + "\n" + src
    modes = [True] if only_strict else [False, True]

    def run(strict):
        prefix = ('"use strict";\n' if strict else "")
        code = prefix + base_code
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
        return ("pass" if ok else "negative-phase-mismatch", path, "")
    else:
        if all(r.returncode == 0 for r in results):
            return ("pass", path, "")
        for r in results:
            if r.returncode != 0:
                err = (r.stderr + r.stdout).strip().split("\n")[0]
                return ("fail", path, err)
        return ("fail", path, "")


def main():
    base = os.path.join(ROOT, "test262", "test", "language")
    files = []
    for dirpath, _, filenames in os.walk(base):
        for f in filenames:
            if f.endswith(".js") and not f.endswith("_FIXTURE.js"):
                files.append(os.path.join(dirpath, f))
    files.sort()
    files = files[:2000]

    samples = {"LeftParen": [], "Comma": [], "identifier": []}
    with cf.ThreadPoolExecutor(max_workers=32) as ex:
        for res, path, err in ex.map(run_one, files):
            if res == "pass":
                continue
            for key in samples:
                if key in err and len(samples[key]) < 6:
                    src = open(path, encoding="utf-8").read()
                    body = re.sub(r"/\*---.*?---\*/", "", src, flags=re.S).strip()
                    samples[key].append((os.path.relpath(path, base), err, body[:250]))

    for key in ["LeftParen", "Comma", "identifier"]:
        print(f"=== {key} samples ===")
        for rel, err, body in samples[key]:
            print(f"  {rel}")
            print(f"    err: {err}")
            print(f"    src: {body}")
            print()


if __name__ == "__main__":
    main()
