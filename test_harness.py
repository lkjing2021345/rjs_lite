import os, re, subprocess, tempfile
ROOT = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(ROOT, "target", "release", "rjs_lite.exe")
STA = open(os.path.join(ROOT, "test262", "harness", "sta.js"), encoding="utf-8").read()
ASSERT = open(os.path.join(ROOT, "test262", "harness", "assert.js"), encoding="utf-8").read()
code = STA + "\n" + ASSERT + "\n" + 'assert.sameValue(1, 1); "ok"'
tf = tempfile.NamedTemporaryFile("w", suffix=".js", delete=False, encoding="utf-8")
tf.write(code)
tf.close()
p = subprocess.run([BIN, tf.name], capture_output=True, text=True, timeout=10)
print("rc:", p.returncode)
print("out:", p.stdout[:300])
print("err:", p.stderr[:300])
os.unlink(tf.name)
