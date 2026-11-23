"""One line per failing suite, from a `cargo test --no-fail-fast` log.

A full workspace run takes minutes, so the log is worth
mining rather than re-running. Groups by source file and prints the first
panic message for each, which is almost always enough to classify the
failure.

Usage:

    python tools/test_failure_digest.py <log>
"""
import io
import re
import sys
from collections import OrderedDict

if len(sys.argv) != 2:
    sys.stdout.reconfigure(encoding='utf-8')
    print(__doc__)
    sys.exit(2)

# A panic message can carry anything the test printed, including box
# drawing and `⚠`. A Windows console is cp1252 by default, so printing
# one raises and the digest dies holding the answer.
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

def read_log(path):
    """PowerShell's `*>` writes UTF-16LE, so sniff before decoding.

    Reading a UTF-16 log as UTF-8 yields text full of NULs that matches
    nothing, and the failure mode is a digest reporting zero panics from
    a log full of them.
    """
    raw = io.open(path, 'rb').read()
    if raw[:2] in (b'\xff\xfe', b'\xfe\xff') or raw[1:2] == b'\x00':
        return raw.decode('utf-16', errors='replace')
    return raw.decode('utf-8', errors='replace')


lines = read_log(sys.argv[1]).replace('\r\n', '\n').split('\n')
seen = OrderedDict()
# ⚠ Cargo wraps its output, so "panicked at" and the path it names are
# often on different lines. Joining each line with the next is what makes
# this find anything at all.
at = re.compile(r"panicked at\s+([^\s:]+):\d+:\d+:")

for i, line in enumerate(lines):
    joined = line.rstrip() + ' ' + (lines[i + 1].strip() if i + 1 < len(lines) else '')
    m = at.search(joined)
    if not m:
        continue
    where = m.group(1).replace('\\', '/').split('/')[-1]
    if not where.endswith('.rs') or where in seen:
        continue
    msg = ' '.join(x.strip() for x in lines[i + 2:i + 5] if x.strip())
    seen[where] = msg[:240]

for where, msg in seen.items():
    print(f'{where}\n    {msg}\n')
print(f'{len(seen)} suite(s) with a panic')
