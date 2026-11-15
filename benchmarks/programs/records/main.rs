use std::collections::HashMap;

struct Entry {
    name: String,
    amount: i64,
    active: bool,
}

fn main() {
    let lines = [" cedar |12|open", "maple|7|closed", " birch |5|open", "cedar|3|open", "oak|11|open", "birch|2|closed"];
    let mut entries = Vec::new();
    for _repeat in 0..2000 {
        for line in lines {
            let fields: Vec<&str> = line.split('|').collect();
            entries.push(Entry {
                name: fields[0].trim().to_string(),
                amount: fields[1].parse().unwrap(),
                active: fields[2] == "open",
            });
        }
    }
    let selected: Vec<&Entry> = entries.iter().filter(|entry| entry.active).collect();
    let adjusted: Vec<Entry> = selected.iter().map(|entry| Entry {
        name: entry.name.clone(),
        amount: entry.amount * 2 + 1,
        active: entry.active,
    }).collect();
    let mut totals = HashMap::new();
    for entry in adjusted {
        let previous = totals.get(&entry.name).copied().unwrap_or(0);
        totals.insert(entry.name, previous + entry.amount);
    }
    let mut names: Vec<&String> = totals.keys().collect();
    names.sort();
    let pieces: Vec<String> = names.iter().map(|name| format!("{}={}", name, totals[*name])).collect();
    println!("bench-v1:{}:{}:{}", entries.len(), selected.len(), pieces.join(","));
}
