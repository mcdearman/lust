use logos::Lexer;
use num_bigint::BigInt;
use num_rational::{BigRational, Rational64};

use crate::{
    interner::InternedString,
    token::{Span, Token, TokenKind},
    T,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Sexpr {
    Atom(Atom),
    List(List),
    Nil,
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    head: Option<Box<Cons>>,
}

impl List {
    pub fn new(head: Option<Box<Cons>>) -> Self {
        Self { head }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cons {
    car: Sexpr,
    cdr: Option<Box<Sexpr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    Number(Number),
    Symbol(InternedString),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Number {
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    // BigFloat(),
    Rational(Rational64),
    BigRational(BigRational),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderError(pub String);

impl ReaderError {
    pub fn new(msg: &str) -> Self {
        Self(msg.to_string())
    }
}

/// Parser is a recursive descent parser for the Lust language.
pub struct Reader<'src> {
    /// The source code to parse.
    src: &'src str,

    /// The [`Lexer`] used to lex the source code.
    logos: Lexer<'src, TokenKind>,

    /// The next token to be consumed.
    peek: Option<Token>,
}

impl<'src> Reader<'src> {
    /// Creates a new [`Parser`].
    pub fn new(src: &'src str) -> Self {
        Self {
            src,
            logos: TokenKind::lexer(src),
            peek: None,
        }
    }

    /// Returns the source code of the token.
    fn text(&self, token: Token) -> &'src str {
        token.lit(&self.src)
    }

    /// Returns the peek token in the stream.
    fn next(&mut self) -> Token {
        if let Some(t) = self.peek.take() {
            t
        } else {
            self.generate()
        }
    }

    /// Returns the next token in the stream without consuming it.
    fn peek(&mut self) -> Token {
        if let Some(t) = self.peek {
            t
        } else {
            let t = self.generate();
            self.peek = Some(t);
            t
        }
    }

    /// Gets the next token from the [`Lexer`].
    fn generate(&mut self) -> Token {
        match self.logos.next().map(|t| (t, self.logos.span())) {
            None => Token {
                kind: T![EOF],
                span: Span::new(0, 0),
            },
            Some((T![;], _)) => self.generate(),
            Some((t, s)) => Token {
                kind: t,
                span: Span::from(s),
            },
        }
    }

    /// Returns true if the next token is of the given kind.
    fn at(&mut self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    /// Consumes the next token if it is of the given kind.
    fn consume(&mut self, expected: TokenKind) {
        let token = self.next();
        assert_eq!(
            token.kind, expected,
            "Expected to consume `{}`, but found `{}`",
            expected, token.kind
        );
    }

    /// Parses the source code into a [`Sexpr`].
    pub fn sexpr(&mut self) -> Result<Sexpr> {
        match self.peek().kind {
            T!['('] => {
                self.consume(T!['(']);
                self.list()
            }
            _ => self.atom(),
        }
    }

    /// Parses a list of [`Sexpr`]s.
    fn list(&mut self) -> Result<Sexpr> {
        if self.at(T![')']) {
            return Ok(Sexpr::Nil);
        }

        let car = Box::new(self.sexpr()?);
        let mut cdr = None;
        let mut rest = vec![];

        while !self.at(T![')']) {
            rest.push(self.sexpr()?)
        }

        self.consume(T![')']);

        for car in rest.into_iter().rev() {
            cdr = Some(Box::new(Sexpr::List(List::new(Some(Box::new(Cons {
                car,
                cdr,
            }))))));
        }

        // Ok(Sexpr::List(List::new(Some(Box::new(Cons {
        //     car: *car,
        //     cdr,
        // })))))
    }

    /// Parses an atom.
    fn atom(&mut self) -> Result<Sexpr> {
        match self.peek().kind {
            lit @ T![int] | lit @ T![float] | lit @ T![ratio] | lit @ T![str] | lit @ T![bool] => {
                let lit_tok = self.next();
                let lit_text = self.text(lit_tok);
                let lit = match lit {
                    T![int] => Lit::Number(Number::Int(lit_text.parse().map_err(|_| {
                        ParserError::new(ParserErrorKind::ParseIntegerError, lit_tok.span)
                    })?)),
                    T![float] => Lit::Number(Number::Float(lit_text.parse().map_err(|_| {
                        ParserError::new(ParserErrorKind::ParseFloatError, lit_tok.span)
                    })?)),
                    T![ratio] => Lit::Number(Number::Rational(lit_text.parse().map_err(|_| {
                        ParserError::new(ParserErrorKind::ParseRationalError, lit_tok.span)
                    })?)),
                    T![str] => Lit::Str(lit_text[1..(lit_text.len() - 1)].to_string()),
                    T![bool] => Lit::Bool(lit_text.parse().expect("invalid bool literal")),
                    _ => unreachable!(),
                };
                Ok(Sexpr::Atom(Atom::Lit(lit)))
            }
            T!['['] => {
                self.consume(T!['[']);
                let mut vec = vec![];
                while !self.at(T![']']) {
                    vec.push(self.sexpr()?);
                }
                self.consume(T![']']);
                Ok(Sexpr::Atom(Atom::Lit(Lit::Vec(vec))))
            }
            T!['{'] => {
                self.consume(T!['{']);
                let mut map = BTreeMap::new();
                while !self.at(T!['}']) {
                    let key = self.sexpr()?;
                    let val = self.sexpr()?;
                    map.insert(key, value);
                }
                self.consume(T!['}']);
                Ok(Sexpr::Atom(Atom::Lit(Lit::HashMap(Rc::new(RefCell::new(
                    map,
                ))))))
            }
            T![ident] => {
                let ident = self.next();
                if self.text(ident).starts_with(":") {
                    Ok(Sexpr::Atom(Atom::Keyword(self.text(ident).to_string())))
                } else {
                    Ok(Sexpr::Atom(Atom::Sym(self.text(ident).to_string())))
                }
            }
            kind => {
                panic!("Unknown start of atom: `{}`", kind);
            }
        }
    }
}