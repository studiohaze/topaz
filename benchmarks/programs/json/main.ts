declare function require(name: string): { readFileSync(path: string, encoding: string): string };
const fs = require("node:fs");

type Row = { name: string; amount: number; active: boolean; tags: unknown[] };
const totals = [0, 0, 0, 0];
for (let repeat = 0; repeat < 20; repeat += 1) {
    const text = fs.readFileSync("input.json", "utf8");
    const rows: Row[] = JSON.parse(text);
    for (const row of rows) {
        const name = row.name;
        const amount = row.amount;
        const active = row.active;
        const tags = row.tags;
        if (active) {
            totals[0] += 1;
            totals[1] += amount;
            totals[2] += [...name.trim()].length;
            totals[3] += tags.length;
        }
    }
}
console.log(`bench-v1:${JSON.stringify(totals)}`);
