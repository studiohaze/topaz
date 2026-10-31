//! Shared interpretation of already-parsed literal text and scalar display.

use std::{fmt, sync::Arc};

mod cooking;
mod ir;
mod numeric;
pub use cooking::*;
pub use ir::*;
pub use numeric::*;

pub fn decode_string_escapes(raw: &str, out: &mut String) -> Result<(), String> {
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('{') => out.push('{'),
            Some('}') => out.push('}'),
            other => {
                return Err(format!(
                    "unsupported escape `\\{}`",
                    other.map(String::from).unwrap_or_default()
                ));
            }
        }
    }
    Ok(())
}

pub fn render_float(x: f64) -> String {
    // Integral floats keep a `.0` below the public decimal boundary so
    // int and float renderings never collide in the common display path.
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        x.to_string()
    }
}

pub const STANDARD_RENDER_FUEL: usize = 100_000;
pub const STANDARD_RENDER_DEPTH: usize = 128;

pub fn render_function_name_into(out: &mut String, name: Option<&str>) {
    out.push_str("<function");
    if let Some(name) = name {
        out.push(' ');
        out.push_str(name);
    }
    out.push('>');
}

/// Already-known display information, not executable expressions or runtime
/// values. Cloning a fact shares its immutable node, including repeated prior
/// bindings. The renderer still charges each occurrence against its budget.
#[derive(Clone)]
pub struct LiteralRender(Arc<LiteralRenderNode>);

enum LiteralRenderNode {
    Text(String),
    Array(Vec<LiteralRender>),
    Record(Vec<(String, LiteralRender)>),
}

impl fmt::Debug for LiteralRender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Parent binding/export Debug must not expand a shared DAG either.
        f.debug_tuple("LiteralRender")
            .field(&self.render())
            .finish()
    }
}

pub fn enter_standard_render(out: &mut String, fuel: &mut usize, depth: usize) -> bool {
    if *fuel == 0 || depth > STANDARD_RENDER_DEPTH {
        out.push_str("...");
        return false;
    }
    *fuel -= 1;
    true
}

/// The array/structural-record display leaf shared by runtime and known facts.
pub fn render_aggregate_into<'a, T>(
    record: bool,
    items: impl IntoIterator<Item = (Option<&'a str>, T)>,
    out: &mut String,
    fuel: &mut usize,
    depth: usize,
    mut render: impl FnMut(T, &mut String, &mut usize, usize) -> bool,
) -> bool {
    out.push_str(if record { "{ " } else { "[" });
    for (i, (name, value)) in items.into_iter().enumerate() {
        if i != 0 {
            out.push_str(", ");
        }
        if let Some(name) = name {
            out.push_str(name);
            out.push_str(": ");
        }
        if !render(value, out, fuel, depth + 1) {
            return false;
        }
    }
    out.push_str(if record { " }" } else { "]" });
    true
}

impl LiteralRender {
    pub fn text(text: String) -> Self {
        Self(Arc::new(LiteralRenderNode::Text(text)))
    }

    pub fn array(items: Vec<Self>) -> Self {
        Self(Arc::new(LiteralRenderNode::Array(items)))
    }

    pub fn record(fields: Vec<(String, Self)>) -> Self {
        Self(Arc::new(LiteralRenderNode::Record(fields)))
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut fuel = STANDARD_RENDER_FUEL;
        self.render_into(&mut out, &mut fuel, 0);
        out
    }

    fn render_into(&self, out: &mut String, fuel: &mut usize, depth: usize) -> bool {
        if !enter_standard_render(out, fuel, depth) {
            return false;
        }
        match self.0.as_ref() {
            LiteralRenderNode::Text(text) => out.push_str(text),
            LiteralRenderNode::Array(items) => {
                return render_aggregate_into(
                    false,
                    items.iter().map(|v| (None, v)),
                    out,
                    fuel,
                    depth,
                    Self::render_into,
                );
            }
            LiteralRenderNode::Record(fields) => {
                return render_aggregate_into(
                    true,
                    fields.iter().map(|(k, v)| (Some(k.as_str()), v)),
                    out,
                    fuel,
                    depth,
                    Self::render_into,
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    #[test]
    fn known_render_uses_standard_unquoted_aggregate_layout_and_budgets() {
        let shape = LiteralRender::record(vec![
            (
                "a".into(),
                LiteralRender::array(vec![
                    LiteralRender::text("1".into()),
                    LiteralRender::text("null".into()),
                    LiteralRender::text("()".into()),
                ]),
            ),
            ("z".into(), LiteralRender::text("line\n{value}".into())),
        ]);
        assert_eq!(shape.render(), "{ a: [1, null, ()], z: line\n{value} }");
        assert_eq!(LiteralRender::array(vec![]).render(), "[]");
        let mut deep = LiteralRender::text("1".into());
        for _ in 0..130 {
            deep = LiteralRender::array(vec![deep]);
        }
        assert_eq!(deep.render(), format!("{}...", "[".repeat(129)));
        let wide = LiteralRender::array(vec![LiteralRender::text("1".into()); 100_000]);
        assert_eq!(wide.render(), format!("[{}...", "1, ".repeat(99_999)));
    }

    #[test]
    fn repeated_fact_edges_share_storage_but_render_each_occurrence() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<LiteralRender>();
        let a0 = LiteralRender::array(vec![LiteralRender::text("0".into())]);
        let a1 = LiteralRender::array(vec![a0.clone(), a0]);
        let a2 = LiteralRender::array(vec![a1.clone(), a1]);
        assert_eq!(a2.render(), "[[[0], [0]], [[0], [0]]]");
        let mut limited = String::new();
        assert!(!a2.render_into(&mut limited, &mut 7, 0));
        assert_eq!(limited, "[[[0], [0]], [...");

        let mut shape = a2;
        let mut nodes = Vec::new();
        for _ in 2..26 {
            nodes.push(Arc::downgrade(&shape.0));
            let alias = shape.clone();
            assert!(Arc::ptr_eq(&shape.0, &alias.0));
            shape = LiteralRender::array(vec![shape, alias]);
        }
        nodes.push(Arc::downgrade(&shape.0));
        drop(shape);
        assert!(nodes.iter().all(|node| node.upgrade().is_none()));
    }
}
