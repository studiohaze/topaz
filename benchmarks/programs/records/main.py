from dataclasses import dataclass


@dataclass
class Entry:
    name: str
    amount: int
    active: bool


lines = [" cedar |12|open", "maple|7|closed", " birch |5|open", "cedar|3|open", "oak|11|open", "birch|2|closed"]
entries = []
for repeat in range(2000):
    for line in lines:
        fields = line.split("|")
        entries.append(Entry(fields[0].strip(), int(fields[1]), fields[2] == "open"))
selected = [entry for entry in entries if entry.active]
adjusted = [Entry(entry.name, entry.amount * 2 + 1, entry.active) for entry in selected]
totals = {}
for entry in adjusted:
    totals[entry.name] = totals.get(entry.name, 0) + entry.amount
pieces = [f"{name}={totals[name]}" for name in sorted(totals)]
print(f"bench-v1:{len(entries)}:{len(selected)}:{','.join(pieces)}")
