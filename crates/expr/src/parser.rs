//! Pratt parser: tokens to an unresolved syntax tree.
//!
//! Binding powers are listed in `infix_bp`; the left number is how tightly an
//! operator holds what is to its left. Giving the right number the smaller
//! value makes an operator right-associative, which is what `^` and `?:` need.

use crate::error::{CompileError, Span};
use crate::expr::{BinaryOp, UnaryOp};
use crate::lexer::{Tok, Token, tokenize};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ast {
    Number(f64),
    Var {
        name: String,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        rhs: Box<Ast>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Ast>,
        rhs: Box<Ast>,
    },
    Ternary {
        cond: Box<Ast>,
        then: Box<Ast>,
        otherwise: Box<Ast>,
    },
    Call {
        name: String,
        args: Vec<Ast>,
        span: Span,
    },
}

/// How tightly a prefix `-` or `!` holds its operand. Below `^` on purpose, so
/// `-2^2` is `-(2^2)`.
const PREFIX_BP: u8 = 15;

const fn infix_bp(tok: &Tok) -> Option<(u8, u8)> {
    Some(match tok {
        Tok::Question => (2, 1),
        Tok::OrOr => (3, 4),
        Tok::AndAnd => (5, 6),
        Tok::EqEq | Tok::Ne => (7, 8),
        Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge => (9, 10),
        Tok::Plus | Tok::Minus => (11, 12),
        Tok::Star | Tok::Slash | Tok::Percent => (13, 14),
        Tok::Caret => (18, 17),
        _ => return None,
    })
}

const fn binary_op(tok: &Tok) -> Option<BinaryOp> {
    Some(match tok {
        Tok::Plus => BinaryOp::Add,
        Tok::Minus => BinaryOp::Sub,
        Tok::Star => BinaryOp::Mul,
        Tok::Slash => BinaryOp::Div,
        Tok::Percent => BinaryOp::Rem,
        Tok::Caret => BinaryOp::Pow,
        Tok::Lt => BinaryOp::Lt,
        Tok::Le => BinaryOp::Le,
        Tok::Gt => BinaryOp::Gt,
        Tok::Ge => BinaryOp::Ge,
        Tok::EqEq => BinaryOp::Eq,
        Tok::Ne => BinaryOp::Ne,
        Tok::AndAnd => BinaryOp::And,
        Tok::OrOr => BinaryOp::Or,
        _ => return None,
    })
}

pub(crate) fn parse(source: &str) -> Result<Ast, CompileError> {
    let tokens = tokenize(source)?;
    let eof = Token {
        tok: Tok::Eof,
        span: Span::new(source.len(), source.len()),
    };
    let mut parser = Parser {
        tokens,
        pos: 0,
        eof,
    };

    if matches!(parser.peek().tok, Tok::Eof) {
        return Err(CompileError::Empty {
            span: parser.peek().span,
        });
    }

    let ast = parser.expr(0)?;

    let trailing = parser.peek();
    if !matches!(trailing.tok, Tok::Eof) {
        return Err(CompileError::UnexpectedToken {
            span: trailing.span,
            found: trailing.tok.to_string(),
            expected: "an operator or the end of the formula",
        });
    }

    Ok(ast)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Returned by `peek` past the end, so it never has to panic or unwrap.
    eof: Token,
}

impl Parser {
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&self.eof)
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if !matches!(token.tok, Tok::Eof) {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, want: &Tok, expected: &'static str) -> Result<Token, CompileError> {
        let token = self.peek().clone();
        if &token.tok == want {
            self.pos += 1;
            return Ok(token);
        }
        Err(if matches!(token.tok, Tok::Eof) {
            CompileError::UnexpectedEnd {
                span: token.span,
                expected,
            }
        } else {
            CompileError::UnexpectedToken {
                span: token.span,
                found: token.tok.to_string(),
                expected,
            }
        })
    }

    /// Parses while the next operator binds at least as tightly as `min_bp`.
    fn expr(&mut self, min_bp: u8) -> Result<Ast, CompileError> {
        let mut lhs = self.prefix()?;

        loop {
            let token = self.peek().clone();
            let Some((left_bp, right_bp)) = infix_bp(&token.tok) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            self.pos += 1;

            if matches!(token.tok, Tok::Question) {
                let then = self.expr(0)?;
                self.eat(&Tok::Colon, "`:` to finish the `? :`")?;
                let otherwise = self.expr(right_bp)?;
                lhs = Ast::Ternary {
                    cond: Box::new(lhs),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                };
                continue;
            }

            let Some(op) = binary_op(&token.tok) else {
                break;
            };
            let rhs = self.expr(right_bp)?;
            lhs = Ast::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Ast, CompileError> {
        let token = self.advance();

        match token.tok {
            Tok::Number(value) => Ok(Ast::Number(value)),

            Tok::Ident(name) => {
                if matches!(self.peek().tok, Tok::LParen) {
                    self.pos += 1;
                    let args = self.call_args()?;
                    let close = self.eat(&Tok::RParen, "`)` to close the argument list")?;
                    Ok(Ast::Call {
                        name,
                        args,
                        span: Span::new(token.span.start, close.span.end),
                    })
                } else {
                    Ok(Ast::Var {
                        name,
                        span: token.span,
                    })
                }
            }

            Tok::Minus => Ok(Ast::Unary {
                op: UnaryOp::Neg,
                rhs: Box::new(self.expr(PREFIX_BP)?),
            }),
            // A leading `+` is accepted and does nothing.
            Tok::Plus => self.expr(PREFIX_BP),
            Tok::Bang => Ok(Ast::Unary {
                op: UnaryOp::Not,
                rhs: Box::new(self.expr(PREFIX_BP)?),
            }),

            Tok::LParen => {
                let inner = self.expr(0)?;
                self.eat(&Tok::RParen, "`)` to close the group")?;
                Ok(inner)
            }

            Tok::Eof => Err(CompileError::UnexpectedEnd {
                span: token.span,
                expected: "a number, a variable or `(`",
            }),

            other => Err(CompileError::UnexpectedToken {
                span: token.span,
                found: other.to_string(),
                expected: "a number, a variable or `(`",
            }),
        }
    }

    fn call_args(&mut self) -> Result<Vec<Ast>, CompileError> {
        let mut args = Vec::new();
        if matches!(self.peek().tok, Tok::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.expr(0)?);
            if matches!(self.peek().tok, Tok::Comma) {
                self.pos += 1;
            } else {
                return Ok(args);
            }
        }
    }
}
