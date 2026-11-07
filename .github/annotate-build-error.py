"""Posts the interesting part of a failed build log as a GitHub annotation.

Job logs need repo access, but annotations are public.
"""

import re
import sys

lines = open(sys.argv[1], encoding="utf-8", errors="replace").read().splitlines()
pattern = re.compile(r"^error|Error |Undefined symbols|panicked|fatal error|^tools:")
wanted = set()
for index, line in enumerate(lines):
    if pattern.search(line):
        wanted.update(range(max(0, index - 3), min(len(lines), index + 30)))
picked = [lines[index] for index in sorted(wanted)]
if not picked:
    picked = lines[-60:]
# Keep it well under GitHub's annotation size limit.
message = "\n".join(picked[-150:])[-30000:]
escaped = message.replace("%", "%25").replace("\r", "").replace("\n", "%0A")
print(f"::error title=Build error::{escaped}")
