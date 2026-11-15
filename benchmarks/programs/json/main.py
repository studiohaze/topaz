import json
from pathlib import Path


totals = [0, 0, 0, 0]
for repeat in range(20):
    text = Path("input.json").read_text(encoding="utf-8")
    rows = json.loads(text)
    for row in rows:
        name = row["name"]
        amount = row["amount"]
        active = row["active"]
        tags = row["tags"]
        if active:
            totals[0] += 1
            totals[1] += amount
            totals[2] += len(name.strip())
            totals[3] += len(tags)
print("bench-v1:" + json.dumps(totals, separators=(",", ":")))
