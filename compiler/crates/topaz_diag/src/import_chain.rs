use std::collections::{BTreeMap, VecDeque};

/// Renders the module-initialization fault suffix shared by interpreter and
/// generated backends. The shortest entry-to-target path is selected by BFS;
/// graph identities stay borrowed from the admitted edge inventory.
pub fn render_import_chain<'a>(
    entry: &'a str,
    edges: &'a [(String, String)],
    target: &str,
) -> String {
    let mut queue = VecDeque::from([entry]);
    let mut previous: BTreeMap<&str, &str> = BTreeMap::new();
    while let Some(current) = queue.pop_front() {
        if current == target {
            let mut chain = vec![current];
            let mut at = current;
            while let Some(parent) = previous.get(at).copied() {
                chain.push(parent);
                at = parent;
            }
            chain.reverse();
            return format!("import chain: {}", chain.join(" -> "));
        }
        for (from, to) in edges {
            if from == current && !previous.contains_key(to.as_str()) && to != entry {
                previous.insert(to, current);
                queue.push_back(to);
            }
        }
    }
    format!("imported from `{entry}`")
}
