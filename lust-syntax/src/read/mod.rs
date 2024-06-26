pub mod sexpr;
pub mod token;

use self::{
    sexpr::{Atom, AtomKind, Lit, Root, Sexpr, SexprKind},
    token::Token,
};
use chumsky::{
    extra,
    input::{Stream, ValueInput},
    prelude::{Input, Rich},
    primitive::just,
    recursive::recursive,
    select, IterParser, Parser,
};
use logos::Logos;
use lust_utils::{intern::InternedString, list::List, span::Span};
use std::vec;

#[derive(Debug, Clone, PartialEq)]
pub enum SyntaxError<'a> {
    LexError(Span),
    ParseError(Rich<'a, Token, Span, &'a str>),
}

pub fn read<'src>(src: &'src str) -> (Option<Root>, Vec<SyntaxError<'src>>) {
    let mut errs = Vec::new();
    let mut tokens = vec![];
    for (res, span) in Token::lexer(src).spanned() {
        match res {
            Ok(tok) => tokens.push((tok, Span::from(span))),
            Err(_) => {
                errs.push(SyntaxError::LexError(Span::from(span.clone())));
                tokens.push((Token::Error, Span::from(span)))
            }
        }
    }
    if !errs.is_empty() {
        return (None, errs);
    }
    println!("tokens: {:?}", tokens);
    let tok_stream = Stream::from_iter(tokens).spanned(Span::from(src.len()..src.len()));
    let (root, errs) = root_reader().parse(tok_stream).into_output_errors();
    (
        root,
        errs.into_iter()
            .map(|err| SyntaxError::ParseError(err))
            .collect(),
    )
}

fn root_reader<'a, I: ValueInput<'a, Token = Token, Span = Span>>(
) -> impl Parser<'a, I, Root, extra::Err<Rich<'a, Token, Span>>> {
    sexpr_reader()
        .repeated()
        .collect()
        .map_with_span(Root::new)
        .boxed()
}

fn sexpr_reader<'a, I: ValueInput<'a, Token = Token, Span = Span>>(
) -> impl Parser<'a, I, Sexpr, extra::Err<Rich<'a, Token, Span>>> {
    recursive(|sexpr| {
        // path = symbol ("." symbol)+
        let path = ident_reader()
            .then(
                just(Token::Period)
                    .ignore_then(ident_reader())
                    .repeated()
                    .at_least(1)
                    .collect::<Vec<_>>(),
            )
            .map(|(lhs, rhs)| {
                let mut v = vec![lhs];
                v.extend(rhs);
                v
            })
            .map(AtomKind::Path);

        let atom = path
            .or(ident_reader().map(AtomKind::Sym))
            .or(lit_reader().map(AtomKind::Lit))
            .map_with_span(Atom::new)
            .map(SexprKind::Atom)
            .map_with_span(Sexpr::new)
            .boxed();

        let list = sexpr
            .clone()
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .map(List::from)
            .map(SexprKind::List)
            .map_with_span(Sexpr::new)
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let list_lit = sexpr
            .clone()
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .map(List::from)
            .map_with_span(|mut list, span: Span| {
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(
                        AtomKind::Sym(InternedString::from("list")),
                        Span::from(span.start()..span.start()),
                    )),
                    span,
                ));
                SexprKind::List(list)
            })
            .map_with_span(Sexpr::new)
            .delimited_by(just(Token::LBrack), just(Token::RBrack));

        let vector = sexpr
            .clone()
            .repeated()
            .collect::<Vec<_>>()
            .map(List::from)
            .map(SexprKind::List)
            .map_with_span(Sexpr::new)
            .delimited_by(just(Token::HashLBrack), just(Token::RBrack));

        // quote = "'" sexpr
        let quote = just(Token::Quote)
            .map_with_span(|_, span| span)
            .then(sexpr.clone())
            .map(|(span, sexpr)| {
                let mut list = List::Empty;
                list.push_front(sexpr);
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(
                        AtomKind::Sym(InternedString::from("quote")),
                        span,
                    )),
                    span,
                ));
                SexprKind::List(list)
            })
            .map_with_span(Sexpr::new);

        let quasiquote = just(Token::Backquote)
            .map_with_span(|_, span| span)
            .then(sexpr.clone())
            .map(|(span, sexpr)| {
                let mut list = List::Empty;
                list.push_front(sexpr);
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(
                        AtomKind::Sym(InternedString::from("quasiquote")),
                        span,
                    )),
                    span,
                ));
                SexprKind::List(list)
            })
            .map_with_span(Sexpr::new);

        let unquote = just(Token::Comma)
            .map_with_span(|_, span| span)
            .then(sexpr.clone())
            .map(|(span, sexpr)| {
                let mut list = List::Empty;
                list.push_front(sexpr);
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(
                        AtomKind::Sym(InternedString::from("unquote")),
                        span,
                    )),
                    span,
                ));
                SexprKind::List(list)
            })
            .map_with_span(Sexpr::new);

        let unquote_splice = just(Token::CommaAt)
            .map_with_span(|_, span| span)
            .then(sexpr.clone())
            .map(|(span, sexpr)| {
                let mut list = List::Empty;
                list.push_front(sexpr);
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(
                        AtomKind::Sym(InternedString::from("unquote-splicing")),
                        span,
                    )),
                    span,
                ));
                SexprKind::List(list)
            })
            .map_with_span(Sexpr::new);

        // map foo... to (vargs foo)
        let variadic = ident_reader()
            .then_ignore(just(Token::Ellipsis))
            .map_with_span(|name, span| {
                let mut list = List::Empty;
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(AtomKind::Sym(name), span)),
                    span,
                ));
                list.push_front(Sexpr::new(
                    SexprKind::Atom(Atom::new(AtomKind::Sym(InternedString::from("varg")), span)),
                    span,
                ));
                SexprKind::List(list)
            })
            .map_with_span(Sexpr::new)
            .boxed();

        variadic
            .or(list)
            .or(list_lit)
            .or(vector)
            .or(quote)
            .or(quasiquote)
            .or(unquote)
            .or(unquote_splice)
            .or(atom)
    })
}

fn ident_reader<'a, I: ValueInput<'a, Token = Token, Span = Span>>(
) -> impl Parser<'a, I, InternedString, extra::Err<Rich<'a, Token, Span>>> {
    select! {
        Token::Ident(name) => name,
    }
}

fn lit_reader<'a, I: ValueInput<'a, Token = Token, Span = Span>>(
) -> impl Parser<'a, I, Lit, extra::Err<Rich<'a, Token, Span>>> {
    select! {
        Token::Int(n) => Lit::Int(n),
        Token::Real(n) => Lit::Real(n),
        Token::Rational(n) => Lit::Rational(n),
        Token::Bool(b) => Lit::Bool(b),
        Token::String(s) => Lit::String(s),
    }
}
