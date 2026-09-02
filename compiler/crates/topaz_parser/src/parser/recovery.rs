use super::*;

impl Parser<'_> {
    /// Panic-mode recovery: skips to the next `Sep` at the current
    /// nesting depth (consumed) or to a closing delimiter / Eof (left
    /// for the enclosing context).
    pub(super) fn synchronize(&mut self) {
        let mut depth = 0u32;
        loop {
            let kind = self.peek();
            if kind == TokenKind::Eof {
                break;
            }
            if kind == TokenKind::Sep && depth == 0 {
                self.bump();
                break;
            }
            if Self::is_opener(kind) {
                depth += 1;
            } else if Self::is_closer(kind) {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            self.bump();
        }
    }

    pub(super) fn skip_seps(&mut self) {
        while self.at(TokenKind::Sep) {
            self.bump();
        }
    }

    /// After an item: consumes the separator run, or accepts a block
    /// close / Eof; anything else is a separation error.
    pub(super) fn item_boundary(&mut self) {
        if self.at(TokenKind::Sep) {
            self.skip_seps();
        } else if !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            self.error_here("expected a statement separator");
            self.synchronize();
        }
    }

    /// Skip from the cursor to the `)` that closes the current parenthesized group
    /// (depth 0), honoring nested delimiters, and consume it — recovery after a
    /// malformed `(a, b, …)` so parsing continues past the close.
    pub(super) fn recover_to_matching_rparen(&mut self) {
        let mut depth = 0u32;
        loop {
            let k = self.peek();
            if k == TokenKind::Eof {
                break;
            }
            if Self::is_opener(k) {
                depth += 1;
            } else if Self::is_closer(k) {
                if depth == 0 {
                    if k == TokenKind::RParen {
                        self.bump();
                    }
                    break;
                }
                depth -= 1;
            }
            self.bump();
        }
    }

    /// Skip malformed record-update content to the `}` that closes the current
    /// update, leaving the close for `record_construct_fields` to consume. This
    /// keeps one structural spread diagnostic from cascading at program scope.
    pub(super) fn recover_to_matching_rbrace(&mut self) {
        let mut depth = 0u32;
        loop {
            let kind = self.peek();
            if kind == TokenKind::Eof {
                break;
            }
            if Self::is_opener(kind) {
                depth += 1;
            } else if Self::is_closer(kind) {
                if depth == 0 {
                    if kind == TokenKind::RBrace {
                        break;
                    }
                    return;
                }
                depth -= 1;
            }
            self.bump();
        }
    }
}
