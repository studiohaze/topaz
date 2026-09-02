use super::*;

/// §11 Regex match payload. Named captures are sorted by name for deterministic
/// render/JSON/equality; unmatched optional named captures are omitted from `named`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatchData {
    pub start: i64,
    pub end: i64,
    pub text: Rc<str>,
    pub groups: Rc<[Option<Rc<str>>]>,
    pub named: Rc<[(Rc<str>, Rc<str>)]>,
}

pub(super) const REGEX_STEP_LIMIT: usize = 100_000;

pub(super) type RegexCapture = Option<(usize, usize)>;
pub(super) type RegexCaptures = Vec<RegexCapture>;
pub(super) type RegexMatchState = Option<(usize, RegexCaptures)>;
pub(super) type UrlQueryPairs = Rc<[(Rc<str>, Rc<str>)]>;

#[derive(Debug, Clone)]
pub struct MiniRegex {
    pub(super) pattern: Rc<str>,
    pub(super) program: RegexProgram,
    pub(super) group_names: Rc<[(Rc<str>, usize)]>,
    pub(super) group_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RegexProgram {
    alternatives: Rc<[Rc<[RegexNode]>]>,
}

#[derive(Debug, Clone)]
pub(super) struct RegexNode {
    atom: RegexAtom,
    quant: RegexQuant,
}

#[derive(Debug, Clone)]
pub(super) enum RegexAtom {
    Literal(char),
    Any,
    Class(CharClass),
    AnchorStart,
    AnchorEnd,
    Group {
        index: Option<usize>,
        program: RegexProgram,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegexQuant {
    One,
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Debug, Clone)]
pub(super) struct CharClass {
    negated: bool,
    items: Rc<[ClassItem]>,
}

#[derive(Debug, Clone)]
pub(super) enum ClassItem {
    One(char),
    Range(char, char),
    Digit,
    Word,
    Space,
}

#[derive(Debug, Clone)]
pub(super) struct MiniMatch {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) groups: RegexCaptures,
}

#[derive(Debug, Clone)]
pub(super) struct RegexParser {
    chars: Vec<char>,
    pos: usize,
    pub(super) group_count: usize,
    names: Vec<(Rc<str>, usize)>,
}

impl MiniRegex {
    pub(super) fn compile(pattern: Rc<str>) -> Result<Self, String> {
        let mut parser = RegexParser {
            chars: pattern.chars().collect(),
            pos: 0,
            group_count: 0,
            names: Vec::new(),
        };
        let program = parser.parse_program(false)?;
        if parser.pos != parser.chars.len() {
            return Err("unexpected trailing regex syntax".to_string());
        }
        Ok(Self {
            pattern,
            program,
            group_names: Rc::from(parser.names.into_boxed_slice()),
            group_count: parser.group_count,
        })
    }

    pub(super) fn as_str(&self) -> &str {
        &self.pattern
    }

    pub(super) fn find_from(
        &self,
        chars: &[char],
        start: usize,
    ) -> Result<Option<MiniMatch>, String> {
        let mut steps = REGEX_STEP_LIMIT;
        for pos in start..=chars.len() {
            let caps = vec![None; self.group_count];
            if let Some((end, groups)) =
                self.match_program(&self.program, chars, pos, caps, &mut steps)?
            {
                return Ok(Some(MiniMatch {
                    start: pos,
                    end,
                    groups,
                }));
            }
            if self.program_is_start_anchored(&self.program) {
                break;
            }
        }
        Ok(None)
    }

    pub(super) fn is_match(&self, text: &str) -> Result<bool, String> {
        let chars: Vec<char> = text.chars().collect();
        Ok(self.find_from(&chars, 0)?.is_some())
    }

    fn match_nodes(
        &self,
        nodes: &[RegexNode],
        idx: usize,
        chars: &[char],
        pos: usize,
        caps: RegexCaptures,
        steps: &mut usize,
    ) -> Result<RegexMatchState, String> {
        if *steps == 0 {
            return Err("Regex: match step limit exceeded".to_string());
        }
        *steps -= 1;
        if idx == nodes.len() {
            return Ok(Some((pos, caps)));
        }
        let node = &nodes[idx];
        let min = match node.quant {
            RegexQuant::One | RegexQuant::OneOrMore => 1,
            RegexQuant::ZeroOrOne | RegexQuant::ZeroOrMore => 0,
        };
        let max = match node.quant {
            RegexQuant::One | RegexQuant::OneOrMore => usize::MAX,
            RegexQuant::ZeroOrOne => 1,
            RegexQuant::ZeroOrMore => usize::MAX,
        };
        let mut states = vec![(pos, caps)];
        let mut count = 0usize;
        while count < max {
            let (p, c) = states.last().cloned().expect("non-empty states");
            let Some((next_p, next_c)) = self.match_atom(&node.atom, chars, p, c, steps)? else {
                break;
            };
            states.push((next_p, next_c));
            count += 1;
            if next_p == p {
                break;
            }
        }
        for (n, (p, c)) in states.into_iter().enumerate().rev() {
            if n < min {
                continue;
            }
            if let Some(done) = self.match_nodes(nodes, idx + 1, chars, p, c, steps)? {
                return Ok(Some(done));
            }
        }
        Ok(None)
    }

    fn match_program(
        &self,
        program: &RegexProgram,
        chars: &[char],
        pos: usize,
        caps: RegexCaptures,
        steps: &mut usize,
    ) -> Result<RegexMatchState, String> {
        for alt in program.alternatives.iter() {
            if let Some(done) = self.match_nodes(alt, 0, chars, pos, caps.clone(), steps)? {
                return Ok(Some(done));
            }
        }
        Ok(None)
    }

    fn program_is_start_anchored(&self, program: &RegexProgram) -> bool {
        !program.alternatives.is_empty()
            && program.alternatives.iter().all(|alt| {
                alt.first()
                    .is_some_and(|n| matches!(n.atom, RegexAtom::AnchorStart))
            })
    }

    fn match_atom(
        &self,
        atom: &RegexAtom,
        chars: &[char],
        pos: usize,
        caps: RegexCaptures,
        steps: &mut usize,
    ) -> Result<RegexMatchState, String> {
        match atom {
            RegexAtom::Literal(ch) => {
                if chars.get(pos) == Some(ch) {
                    Ok(Some((pos + 1, caps)))
                } else {
                    Ok(None)
                }
            }
            RegexAtom::Any => match chars.get(pos) {
                Some('\n') | None => Ok(None),
                Some(_) => Ok(Some((pos + 1, caps))),
            },
            RegexAtom::Class(class) => match chars.get(pos) {
                Some(ch) if class.matches(*ch) => Ok(Some((pos + 1, caps))),
                _ => Ok(None),
            },
            RegexAtom::AnchorStart => {
                if pos == 0 {
                    Ok(Some((pos, caps)))
                } else {
                    Ok(None)
                }
            }
            RegexAtom::AnchorEnd => {
                if pos == chars.len() {
                    Ok(Some((pos, caps)))
                } else {
                    Ok(None)
                }
            }
            RegexAtom::Group { index, program } => {
                let start = pos;
                let Some((end, mut inner_caps)) =
                    self.match_program(program, chars, pos, caps, steps)?
                else {
                    return Ok(None);
                };
                if let Some(i) = index {
                    inner_caps[*i - 1] = Some((start, end));
                }
                Ok(Some((end, inner_caps)))
            }
        }
    }
}

impl CharClass {
    fn matches(&self, ch: char) -> bool {
        let hit = self.items.iter().any(|item| match item {
            ClassItem::One(x) => *x == ch,
            ClassItem::Range(a, b) => *a <= ch && ch <= *b,
            ClassItem::Digit => ch.is_ascii_digit(),
            ClassItem::Word => ch == '_' || ch.is_ascii_alphanumeric(),
            ClassItem::Space => matches!(ch, ' ' | '\t' | '\n' | '\r'),
        });
        if self.negated { !hit } else { hit }
    }
}

impl RegexParser {
    fn parse_program(&mut self, in_group: bool) -> Result<RegexProgram, String> {
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(Rc::from(self.parse_sequence(in_group)?.into_boxed_slice()));
            match self.peek() {
                Some('|') => {
                    self.pos += 1;
                }
                Some(')') if in_group => {
                    self.pos += 1;
                    break;
                }
                Some(')') => return Err("unmatched `)`".to_string()),
                None if in_group => return Err("unclosed group".to_string()),
                _ => break,
            }
        }
        Ok(RegexProgram {
            alternatives: Rc::from(alternatives.into_boxed_slice()),
        })
    }

    fn parse_sequence(&mut self, in_group: bool) -> Result<Vec<RegexNode>, String> {
        let mut nodes = Vec::new();
        while self.pos < self.chars.len() {
            if self.peek() == Some('|') || (in_group && self.peek() == Some(')')) {
                return Ok(nodes);
            }
            if !in_group && self.peek() == Some(')') {
                return Err("unmatched `)`".to_string());
            }
            let atom = self.parse_atom()?;
            let quant = match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    RegexQuant::ZeroOrMore
                }
                Some('+') => {
                    self.pos += 1;
                    RegexQuant::OneOrMore
                }
                Some('?') => {
                    self.pos += 1;
                    RegexQuant::ZeroOrOne
                }
                _ => RegexQuant::One,
            };
            if quant != RegexQuant::One
                && matches!(atom, RegexAtom::AnchorStart | RegexAtom::AnchorEnd)
            {
                return Err("anchors cannot be quantified".to_string());
            }
            nodes.push(RegexNode { atom, quant });
        }
        Ok(nodes)
    }

    fn parse_atom(&mut self) -> Result<RegexAtom, String> {
        let ch = self
            .next()
            .ok_or_else(|| "unexpected end of pattern".to_string())?;
        match ch {
            '^' => Ok(RegexAtom::AnchorStart),
            '$' => Ok(RegexAtom::AnchorEnd),
            '.' => Ok(RegexAtom::Any),
            '[' => self.parse_class(),
            '(' => self.parse_group(),
            '*' | '+' | '?' => Err(format!("quantifier `{ch}` has no target")),
            '\\' => Ok(self.parse_escape_atom()?),
            ')' | '|' => unreachable!("handled by parse_sequence"),
            c => Ok(RegexAtom::Literal(c)),
        }
    }

    fn parse_group(&mut self) -> Result<RegexAtom, String> {
        let index = if self.peek() == Some('?') {
            self.pos += 1;
            match self.next() {
                Some(':') => None,
                Some('P') if self.next() == Some('<') => Some(self.parse_named_group_index()?),
                Some('<') => Some(self.parse_named_group_index()?),
                _ => return Err("unsupported group syntax".to_string()),
            }
        } else {
            self.group_count += 1;
            Some(self.group_count)
        };
        let program = self.parse_program(true)?;
        Ok(RegexAtom::Group { index, program })
    }

    fn parse_named_group_index(&mut self) -> Result<usize, String> {
        let start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos] != '>' {
            self.pos += 1;
        }
        if self.pos >= self.chars.len() {
            return Err("unclosed named group".to_string());
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        self.pos += 1;
        if name.is_empty()
            || !name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
            || name.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            return Err("invalid named group".to_string());
        }
        if self
            .names
            .iter()
            .any(|(existing, _)| existing.as_ref() == name)
        {
            return Err("duplicate named group".to_string());
        }
        self.group_count += 1;
        self.names.push((Rc::from(name), self.group_count));
        Ok(self.group_count)
    }

    fn parse_class(&mut self) -> Result<RegexAtom, String> {
        let negated = if self.peek() == Some('^') {
            self.pos += 1;
            true
        } else {
            false
        };
        let mut items = Vec::new();
        let mut first = true;
        while let Some(ch) = self.peek() {
            if ch == ']' && !first {
                self.pos += 1;
                return Ok(RegexAtom::Class(CharClass {
                    negated,
                    items: Rc::from(items.into_boxed_slice()),
                }));
            }
            first = false;
            let item = self.parse_class_item()?;
            if self.peek() == Some('-')
                && self.chars.get(self.pos + 1).is_some_and(|c| *c != ']')
                && let ClassItem::One(start) = item
            {
                self.pos += 1;
                let end_item = self.parse_class_item()?;
                let ClassItem::One(end) = end_item else {
                    return Err("character-class ranges need literal endpoints".to_string());
                };
                if start > end {
                    return Err("character-class range is inverted".to_string());
                }
                items.push(ClassItem::Range(start, end));
            } else {
                items.push(item);
            }
        }
        Err("unclosed character class".to_string())
    }

    fn parse_class_item(&mut self) -> Result<ClassItem, String> {
        match self.next() {
            Some('\\') => match self.next() {
                Some('d') => Ok(ClassItem::Digit),
                Some('w') => Ok(ClassItem::Word),
                Some('s') => Ok(ClassItem::Space),
                Some(c) if c.is_ascii_alphabetic() && !matches!(c, 'n' | 'r' | 't') => {
                    Err(format!("unsupported escape `\\{c}`"))
                }
                Some(c) => Ok(ClassItem::One(Self::escaped_literal(c))),
                None => Err("dangling escape in character class".to_string()),
            },
            Some(c) => Ok(ClassItem::One(c)),
            None => Err("unclosed character class".to_string()),
        }
    }

    fn parse_escape_atom(&mut self) -> Result<RegexAtom, String> {
        match self.next() {
            Some('d') => Ok(RegexAtom::Class(CharClass {
                negated: false,
                items: Rc::from([ClassItem::Digit]),
            })),
            Some('w') => Ok(RegexAtom::Class(CharClass {
                negated: false,
                items: Rc::from([ClassItem::Word]),
            })),
            Some('s') => Ok(RegexAtom::Class(CharClass {
                negated: false,
                items: Rc::from([ClassItem::Space]),
            })),
            Some(c) if c.is_ascii_alphabetic() && !matches!(c, 'n' | 'r' | 't') => {
                Err(format!("unsupported escape `\\{c}`"))
            }
            Some(c) => Ok(RegexAtom::Literal(Self::escaped_literal(c))),
            None => Err("dangling escape".to_string()),
        }
    }

    fn escaped_literal(c: char) -> char {
        match c {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            other => other,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }
}
