//! Attribute macros that compile an `async fn`'s body into an async block,
//! as `#[async_trait]` and `#[tracing::instrument]` do, with only the
//! compiler's `proc_macro`. The body keeps its spans, so its code keeps its
//! lines.

use proc_macro::{Delimiter, Group, TokenStream, TokenTree};

/// `async fn f(..) -> R { body }` as
/// `fn f(..) -> Pin<Box<dyn Future<Output = R>>> { Box::pin(async move { body }) }`.
#[proc_macro_attribute]
#[cfg_attr(any(), mutants::skip)]
pub fn boxed(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let (head, ret, body) = split(item);
    let mut out: TokenStream = head.into_iter().collect();
    out.extend("-> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ".parse::<TokenStream>());
    out.extend(ret);
    out.extend(">>>".parse::<TokenStream>());
    let mut inner: TokenStream = "Box::pin".parse().unwrap();
    let mut block: TokenStream = "async move".parse().unwrap();
    block.extend([TokenTree::Group(body)]);
    inner.extend([TokenTree::Group(Group::new(Delimiter::Parenthesis, block))]);
    out.extend([TokenTree::Group(Group::new(Delimiter::Brace, inner))]);
    out
}

/// `async fn f(..) -> R { body }` as `async fn f(..) -> R { async move { body }.await }`.
#[proc_macro_attribute]
#[cfg_attr(any(), mutants::skip)]
pub fn wrapped(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let (head, ret, body) = split(item);
    let mut out = TokenStream::new();
    for t in head {
        if matches!(&t, TokenTree::Ident(i) if i.to_string() == "fn") {
            out.extend("async".parse::<TokenStream>());
        }
        out.extend([t]);
    }
    out.extend("->".parse::<TokenStream>());
    out.extend(ret);
    let mut inner: TokenStream = "async move".parse().unwrap();
    inner.extend([TokenTree::Group(body)]);
    inner.extend(".await".parse::<TokenStream>());
    out.extend([TokenTree::Group(Group::new(Delimiter::Brace, inner))]);
    out
}

/// The item without `async`, up to `->`; its result type; its body.
#[cfg_attr(any(), mutants::skip)]
fn split(item: TokenStream) -> (Vec<TokenTree>, Vec<TokenTree>, Group) {
    let mut tokens: Vec<TokenTree> = item.into_iter().collect();
    let Some(TokenTree::Group(body)) = tokens.pop() else {
        panic!("a fn with a body");
    };
    tokens.retain(|t| !matches!(t, TokenTree::Ident(i) if i.to_string() == "async"));
    let arrow = tokens
        .windows(2)
        .position(|w| matches!(w, [TokenTree::Punct(a), TokenTree::Punct(b)] if a.as_char() == '-' && b.as_char() == '>'))
        .expect("a fn with a result type");
    let ret = tokens.split_off(arrow + 2);
    tokens.truncate(arrow);
    (tokens, ret, body)
}
