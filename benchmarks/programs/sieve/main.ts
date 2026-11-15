const limit = 100000;
const composite: boolean[] = Array(limit + 1).fill(false);
let candidate = 2;
while (candidate * candidate <= limit) {
    if (!composite[candidate]) {
        let multiple = candidate * candidate;
        while (multiple <= limit) {
            composite[multiple] = true;
            multiple += candidate;
        }
    }
    candidate += 1;
}
let count = 0;
let total = 0;
for (let value = 2; value <= limit; value += 1) {
    if (!composite[value]) {
        count += 1;
        total += value;
    }
}
console.log(`bench-v1:${count}:${total}`);
