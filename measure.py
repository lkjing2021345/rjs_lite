import os, re, subprocess, tempfile, collections, concurrent.futures, sys

BIN = os.path.join('target', 'release', 'rjs_lite.exe')
TEST_DIR = os.path.join('test262', 'test', 'language')
HARNESS = os.path.join('test262', 'harness')
STA = open(os.path.join(HARNESS, 'sta.js'), encoding='utf-8').read()
ASSERT = open(os.path.join(HARNESS, 'assert.js'), encoding='utf-8').read()
FM = re.compile(r'/\*---(.*?)---\*/', re.S)
LIMIT = int(sys.argv[1]) if len(sys.argv) > 1 else 3000

inc = {}


def load_include(n):
    if n not in inc:
        try:
            inc[n] = open(os.path.join(HARNESS, n), encoding='utf-8').read()
        except OSError:
            inc[n] = ''
    return inc[n]


def meta(src):
    m = FM.search(src)
    fl, incs, neg = [], [], False
    if m:
        b = m.group(1)
        fm = re.search(r'flags:\s*\[(.*?)\]', b)
        if fm:
            fl = [x.strip() for x in fm.group(1).split(',') if x.strip()]
        im = re.search(r'includes:\s*\[(.*?)\]', b, re.S)
        if im:
            incs = [x.strip() for x in im.group(1).split(',') if x.strip()]
        neg = 'negative:' in b
    return fl, incs, neg


def run(path):
    src = open(path, encoding='utf-8').read()
    fl, incs, neg = meta(src)
    if 'raw' in fl:
        code = src
    else:
        parts = [STA, ASSERT]
        if 'async' in fl:
            parts.append(load_include('doneprintHandle.js'))
        for i in incs:
            parts.append(load_include(i))
        parts.append(src)
        code = '\n'.join(parts)
    tf = tempfile.NamedTemporaryFile('w', suffix='.js', delete=False, encoding='utf-8')
    try:
        tf.write(code)
        tf.close()
        try:
            p = subprocess.run([BIN, tf.name], capture_output=True, text=True, timeout=10)
        except subprocess.TimeoutExpired:
            return (path, 'timeout', '', neg)
        if p.returncode != 0:
            err = (p.stderr or '').strip()
            first = err.splitlines()[0] if err else ''
            msg = re.sub(r'at \d+:\d+: ', '', first)
            phase = 'parse' if err.startswith('SyntaxError:') else 'runtime'
            return (path, phase, msg, neg)
        return (path, 'pass', '', neg)
    finally:
        try:
            os.unlink(tf.name)
        except OSError:
            pass


files = []
for dp, dn, fn in os.walk(TEST_DIR):
    for f in fn:
        if f.endswith('.js') and not f.endswith('_FIXTURE.js'):
            files.append(os.path.join(dp, f))
files.sort()
files = files[:LIMIT]

results = []
with concurrent.futures.ThreadPoolExecutor(32) as ex:
    for res in ex.map(run, files):
        results.append(res)

cnt = collections.Counter()
real_bugs = []
for path, phase, msg, neg in results:
    cnt[phase] += 1
    if phase == 'parse' and not neg:
        real_bugs.append(msg)
print('phases:', dict(cnt))
print('REAL parse bugs (non-negative):', len(real_bugs))
c = collections.Counter(real_bugs)
for m, n in c.most_common(10):
    print(f'  {n:4d}  {m}')
