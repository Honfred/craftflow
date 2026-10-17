//! Turns formula source into tokens, each carrying its span.

use std::fmt;

use crate::error::{CompileError, Span};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Number(f64),
    /// An identifier, possibly dotted: `speed_upgrades`, `recipe.duration`.
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    AndAnd,
    OrOr,
    Bang,
    Question,
    Colon,
    Comma,
    LParen,
    RParen,
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Ident(name) => write!(f, "{name}"),
            Self::Plus => f.write_str("+"),
            Self::Minus => f.write_str("-"),
            Self::Star => f.write_str("*"),
            Self::Slash => f.write_str("/"),
            Self::Percent => f.write_str("%"),
            Self::Caret => f.write_str("^"),
            Self::Lt => f.write_str("<"),
            Self::Le => f.write_str("<="),
            Self::Gt => f.write_str(">"),
            Self::Ge => f.write_str(">="),
            Self::EqEq => f.write_str("=="),
            Self::Ne => f.write_str("!="),
            Self::AndAnd => f.write_str("&&"),
            Self::OrOr => f.write_str("||"),
            Self::Bang => f.write_str("!"),
            Self::Question => f.write_str("?"),
            Self::Colon => f.write_str(":"),
            Self::Comma => f.write_str(","),
            Self::LParen => f.write_str("("),
            Self::RParen => f.write_str(")"),
            Self::Eof => f.write_str("end of formula"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub(crate) fn tokenize(source: &str) -> Result<Vec<Token>, CompileError> {
    let bytes: Vec<(usize, char)> = source.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let (start, ch) = bytes[i];

        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        // A two-character operator, where the second character matters.
        let next = bytes.get(i + 1).map(|&(_, c)| c);
        let two = |tok: Tok, end: usize| Token {
            tok,
            span: Span::new(start, end),
        };

        let single = match ch {
            '0'..='9' | '.' => {
                let (tok, end) = lex_number(source, &bytes, &mut i)?;
                out.push(Token {
                    tok,
                    span: Span::new(start, end),
                });
                continue;
            }
            c if c.is_alphabetic() || c == '_' => {
                let (tok, end) = lex_ident(source, &bytes, &mut i);
                out.push(Token {
                    tok,
                    span: Span::new(start, end),
                });
                continue;
            }
            '<' if next == Some('=') => {
                i += 2;
                out.push(two(Tok::Le, start + 2));
                continue;
            }
            '>' if next == Some('=') => {
                i += 2;
                out.push(two(Tok::Ge, start + 2));
                continue;
            }
            '=' if next == Some('=') => {
                i += 2;
                out.push(two(Tok::EqEq, start + 2));
                continue;
            }
            '!' if next == Some('=') => {
                i += 2;
                out.push(two(Tok::Ne, start + 2));
                continue;
            }
            '&' if next == Some('&') => {
                i += 2;
                out.push(two(Tok::AndAnd, start + 2));
                continue;
            }
            '|' if next == Some('|') => {
                i += 2;
                out.push(two(Tok::OrOr, start + 2));
                continue;
            }
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '%' => Tok::Percent,
            '^' => Tok::Caret,
            '<' => Tok::Lt,
            '>' => Tok::Gt,
            '!' => Tok::Bang,
            '?' => Tok::Question,
            ':' => Tok::Colon,
            ',' => Tok::Comma,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            other => {
                return Err(CompileError::UnexpectedChar {
                    span: Span::new(start, start + other.len_utf8()),
                    ch: other,
                });
            }
        };

        i += 1;
        out.push(Token {
            tok: single,
            span: Span::new(start, start + ch.len_utf8()),
        });
    }

    let end = source.len();
    out.push(Token {
        tok: Tok::Eof,
        span: Span::new(end, end),
    });
    Ok(out)
}

/// Numbers: `2`, `2.5`, `1e-3`, `.5`.
fn lex_number(
    source: &str,
    bytes: &[(usize, char)],
    i: &mut usize,
) -> Result<(Tok, usize), CompileError> {
    let start = bytes[*i].0;
    let mut seen_dot = false;

    while let Some(&(_, ch)) = bytes.get(*i) {
        match ch {
            '0'..='9' => *i += 1,
            '.' if !seen_dot => {
                seen_dot = true;
                *i += 1;
            }
            'e' | 'E' => {
                // Only part of the number when an exponent actually follows.
                let after = bytes.get(*i + 1).map(|&(_, c)| c);
                let digits_follow = match after {
                    Some('+' | '-') => bytes.get(*i + 2).is_some_and(|&(_, c)| c.is_ascii_digit()),
                    Some(c) => c.is_ascii_digit(),
                    None => false,
                };
                if !digits_follow {
                    break;
                }
                *i += if matches!(after, Some('+' | '-')) {
                    2
                } else {
                    1
                };
            }
            _ => break,
        }
    }

    let end = bytes.get(*i).map_or(source.len(), |&(offset, _)| offset);
    let text = &source[start..end];
    let span = Span::new(start, end);

    text.parse::<f64>()
        .map(|value| (Tok::Number(value), end))
        .map_err(|_| CompileError::InvalidNumber {
            span,
            text: text.to_owned(),
        })
}

/// Identifiers, including dotted paths like `recipe.duration`, which the
/// engine treats as one variable name rather than a field access.
fn lex_ident(source: &str, bytes: &[(usize, char)], i: &mut usize) -> (Tok, usize) {
    let start = bytes[*i].0;

    while let Some(&(_, ch)) = bytes.get(*i) {
        if ch.is_alphanumeric() || ch == '_' {
            *i += 1;
        } else if ch == '.' {
            // Only swallow the dot when a name follows it.
            let follows = bytes
                .get(*i + 1)
                .is_some_and(|&(_, c)| c.is_alphabetic() || c == '_');
            if !follows {
                break;
            }
            *i += 1;
        } else {
            break;
        }
    }

    let end = bytes.get(*i).map_or(source.len(), |&(offset, _)| offset);
    (Tok::Ident(source[start..end].to_owned()), end)
}

#[cfg(test)]
mod tests {
    use super::{Tok, tokenize};
    use crate::error::CompileError;

    fn toks(source: &str) -> Vec<Tok> {
        tokenize(source)
            .expect("tokenizes")
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    #[test]
    fn dotted_identifiers_are_one_token() {
        assert_eq!(
            toks("recipe.duration"),
            [Tok::Ident("recipe.duration".to_owned()), Tok::Eof]
        );
    }

    #[test]
    fn numbers_cover_decimals_and_exponents() {
        assert_eq!(toks("2"), [Tok::Number(2.0), Tok::Eof]);
        assert_eq!(toks("2.5"), [Tok::Number(2.5), Tok::Eof]);
        assert_eq!(toks(".5"), [Tok::Number(0.5), Tok::Eof]);
        assert_eq!(toks("1e-3"), [Tok::Number(0.001), Tok::Eof]);
    }

    #[test]
    fn two_character_operators_win_over_one() {
        assert_eq!(
            toks("a<=b"),
            [
                Tok::Ident("a".to_owned()),
                Tok::Le,
                Tok::Ident("b".to_owned()),
                Tok::Eof
            ]
        );
        assert_eq!(
            toks("a<b"),
            [
                Tok::Ident("a".to_owned()),
                Tok::Lt,
                Tok::Ident("b".to_owned()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn spans_point_at_the_right_characters() {
        let tokens = tokenize("ab + cd").expect("tokenizes");
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 2);
        assert_eq!(tokens[2].span.start, 5);
        assert_eq!(tokens[2].span.end, 7);
    }

    #[test]
    fn stray_characters_are_reported_with_a_span() {
        let err = tokenize("a $ b").expect_err("rejects");
        assert!(matches!(
            err,
            CompileError::UnexpectedChar { ch: '$', span } if span.start == 2
        ));
    }
}
