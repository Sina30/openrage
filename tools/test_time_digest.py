"""Slowest test binaries, from a `cargo test` log.

A full workspace run is minutes long and the cost is never spread
evenly — it is a handful of fixture-heavy suites. This pairs each
`Running tests\\<name>.rs` line with the `finished in Ns` that follows it,
so the expensive ones can be found rather than guessed at.

Usage:

    python tools/test_time_digest.py <log> [top-n]
"""
import io
import re
import sys


def read_log(path):
    """PowerShell's `*>` writes UTF-16LE; sniff before decoding."""
    raw = io.open(path, 'rb').read()
    if raw[:2] in (b'\xff\xfe', b'\xfe\xff') or raw[1:2] == b'\x00':
        return raw.decode('utf-16', errors='replace')
    return raw.decode('utf-8', errors='replace')


if len(sys.argv) < 2:
    sys.stdout.reconfigure(encoding='utf-8')
    print(__doc__)
    sys.exit(2)

sys.stdout.reconfigure(encoding='utf-8')
top = int(sys.argv[2]) if len(sys.argv) > 2 else 20

text = read_log(sys.argv[1]).replace('\r\n', '\n')
running = re.compile(r'Running (?:tests|unittests)[\\/ ]?(\S+)')
finished = re.compile(r'finished in ([\d.]+)s')

rows, current = [], None
for line in text.split('\n'):
    m = running.search(line)
    if m:
        current = m.group(1).strip()
        continue
    f = finished.search(line)
    if f and current:
        rows.append((float(f.group(1)), current))
        current = None

rows.sort(reverse=True)
total = sum(t for t, _ in rows)
print(f'{len(rows)} suite(s), {total:.1f}s of reported test time\n')
for seconds, name in rows[:top]:
    share = 100 * seconds / total if total else 0
    print(f'{seconds:8.2f}s  {share:5.1f}%  {name}')
