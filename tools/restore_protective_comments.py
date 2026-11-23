"""Put back the comment blocks a strip removed that no code can replace.

Three categories qualify, and each is a thing the next reader cannot recover
from the lines underneath, from `git log`, or from running the tests:

  measurement  a figure with its denominator -- "13 of 1,262 drawable bases",
               "2,648 event emitters across 4 of 4 .ypt" -- which exists only
               because somebody ran it over a corpus that is not in the tree.
               A size in bytes is not one: that is the layout, and the
               declaration owns it.
  reversal     a claim this codebase tested and refuted, or a design it built
               and reverted. Current code records the outcome and never the
               road not taken.
  trap         a warning carrying its consequence: what breaks, not that care
               is needed.
  doctest      a fenced example rustdoc actually runs. AGENTS.md keeps these
               by name -- "`# Example` only as a doctest that runs" -- and a
               ```text block is not one.

Selection is by category, never by length. AGENTS.md: "Judge a comment by
what it says, never by its length."

    python tools/restore_protective_comments.py origin/main
    python tools/restore_protective_comments.py origin/main --apply
"""

import difflib
import re
import subprocess
import sys

COMMENT = re.compile(r"^\s*(///|//!|//|\*|/\*)")

MEASUREMENT = [
    r"\b\d[\d,]*\s+of\s+\d[\d,]*\b",                      # 13 of 1,262
    r"\b\d{1,3}(,\d{3})+\b",                              # 185,405,296
    r"\bmeasured\b[^.]{0,80}\b(over|across|against)\b",
    r"\b(corpus|census)\b[^.]{0,80}\b\d{2,}\b",
    r"\b\d{3,}\s*(files?|textures?|emitters?|entries|sites|bases|drawables?|clips?|resources?)\b",
]

REVERSAL = [
    r"\brefut(ed|es)\b",
    r"\b(built and reverted|tried and removed|was reverted|reverted because)\b",
    r"\bno longer (holds|true)\b",
    r"\b(used to say|used to be read as|that claim is|turned out to be|proved false)\b",
    r"\brejected alternative\b",
    r"\b(it was|is) circular\b",
]

CONSEQUENCE = (
    r"\b(explodes?|crash(es)?|corrupts?|silently|data loss|panics?|"
    r"undefined behaviour|undefined behavior|stops? (opening|reading)|"
    r"reads? back short|shrinks? an allocation|leaves? a pointer|"
    r"went in and none came out|deadlock|wrong bytes|breaks?)\b"
)

# A banner or a scaffolding heading is furniture whatever else it trips, and
# leaving it in lets one "0x40 bytes" drag a whole layout tour back with it.
FURNITURE = re.compile(
    r"^\s*(#+\s*(Features|Usage|Arguments|Returns|Panics|Errors|Safety|Key Types|"
    r"Module Purpose|Source Reference|Binary Layout|Overview|Layout)\b|[-=]{10,})",
    re.M,
)


# rustdoc runs a fence with no language, or one tagged rust/no_run/
# should_panic/compile_fail. `text`, `ignore`, `console` and the rest are
# prose in a box and carry no coverage.
FENCE = re.compile(r"^\s*```(.*)$")
RUNNABLE = re.compile(r"^\s*(rust|no_run|should_panic|compile_fail|edition\d+)?\s*$")


def has_doctest(text):
    # Only an *opening* fence carries the language, and a closing ``` looks
    # exactly like an untagged opener -- which is how this first matched every
    # ```text block in the tree.
    inside = False
    for line in text.splitlines():
        m = FENCE.match(line)
        if not m:
            continue
        if inside:
            inside = False
            continue
        inside = True
        if RUNNABLE.match(m.group(1)):
            return True
    return False


def classify(lines):
    text = re.sub(r"^\s*(///|//!|//|\*/|\*|/\*)\s?", "", "".join(lines), flags=re.M)
    body = FURNITURE.sub("", text)
    hits = []
    if any(re.search(p, body, re.I) for p in MEASUREMENT):
        hits.append("measurement")
    if any(re.search(p, body, re.I) for p in REVERSAL):
        hits.append("reversal")
    if "\u26a0" in body and re.search(CONSEQUENCE, body, re.I):
        hits.append("trap")
    if has_doctest(text):
        hits.append("doctest")
    return hits


def is_comment_run(lines):
    return lines and all(COMMENT.match(l) or not l.strip() for l in lines)


def changed_files(base):
    out = subprocess.run(
        ["git", "diff", "--name-only", base],
        capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout.split("\n")
    return [p for p in (p.strip() for p in out) if p.endswith(".rs")]


def at(base, path):
    r = subprocess.run(
        ["git", "show", f"{base}:{path}"],
        capture_output=True, encoding="utf-8", errors="replace",
    )
    return None if r.returncode else r.stdout.splitlines(keepends=True)


def restore(base, path, apply_it):
    before = at(base, path)
    if before is None:
        return None
    with open(path, encoding="utf-8", errors="replace", newline="") as fh:
        now = fh.read()
    eol = "\r\n" if "\r\n" in now else "\n"
    now_lines = [l + "\n" for l in now.replace("\r\n", "\n").split("\n")]
    if now_lines and now_lines[-1] == "\n" and not now.endswith("\n"):
        now_lines.pop()
    elif now.endswith("\n"):
        now_lines.pop()

    out, picked = [], []
    sm = difflib.SequenceMatcher(None, before, now_lines, autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag in ("equal", "insert", "replace"):
            out.extend(now_lines[j1:j2])
            if tag != "replace":
                continue
        if tag in ("delete", "replace"):
            run = before[i1:i2]
            if not is_comment_run(run):
                continue
            cats = classify(run)
            if not cats:
                continue
            # A doc comment must sit immediately above its item, so a restored
            # run goes back where it was rather than after what replaced it.
            anchor = len(out) - (j2 - j1) if tag == "replace" else len(out)
            out[anchor:anchor] = run
            picked.append((cats, len(run)))

    if picked and apply_it:
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write("".join(l.rstrip("\n") + eol for l in out))
    return picked


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else "origin/main"
    apply_it = "--apply" in sys.argv

    blocks = lines = 0
    per_cat, per_file = {}, {}
    for path in changed_files(base):
        picked = restore(base, path, apply_it)
        if not picked:
            continue
        per_file[path] = len(picked)
        for cats, n in picked:
            blocks += 1
            lines += n
            for c in cats:
                per_cat[c] = per_cat.get(c, 0) + 1

    print(f"{'restored' if apply_it else 'would restore'}: "
          f"{blocks} comment blocks, {lines} lines, across {len(per_file)} files")
    for c, n in sorted(per_cat.items(), key=lambda kv: -kv[1]):
        print(f"  {c:<12} {n}")
    for path, n in sorted(per_file.items(), key=lambda kv: -kv[1])[:15]:
        print(f"    {n:>3}  {path}")


if __name__ == "__main__":
    main()
