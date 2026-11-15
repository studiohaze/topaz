type Entry = { name: string; amount: number; active: boolean };

const lines = [" cedar |12|open", "maple|7|closed", " birch |5|open", "cedar|3|open", "oak|11|open", "birch|2|closed"];
const entries: Entry[] = [];
for (let repeat = 0; repeat < 2000; repeat += 1) {
    for (const line of lines) {
        const fields = line.split("|");
        entries.push({
            name: fields[0].trim(),
            amount: Number.parseInt(fields[1], 10),
            active: fields[2] === "open",
        });
    }
}
const selected = entries.filter(entry => entry.active);
const adjusted = selected.map(entry => ({
    name: entry.name,
    amount: entry.amount * 2 + 1,
    active: entry.active,
}));
const totals = new Map<string, number>();
for (const entry of adjusted) {
    totals.set(entry.name, (totals.get(entry.name) ?? 0) + entry.amount);
}
const pieces = [...totals.keys()].sort().map(name => `${name}=${totals.get(name)}`);
console.log(`bench-v1:${entries.length}:${selected.length}:${pieces.join(",")}`);
