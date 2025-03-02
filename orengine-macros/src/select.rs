use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Expr, Ident, Token};

pub(crate) struct SelectInput {
    pub(crate) branches: Vec<Branch>,
    pub(crate) default: Option<Expr>,
}

pub(crate) enum Branch {
    Recv {
        channel: Expr,
        var: Ident,
        body: Expr,
    },
    Send {
        channel: Expr,
        value: Expr,
        var: Ident,
        body: Expr,
    },
}

impl Parse for SelectInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut branches = Vec::new();
        let mut default = None;

        while !input.is_empty() {
            let ident: Ident = input.parse().map_err(|_| {
                syn::Error::new(input.span(), "expected `recv`, `send`, or `default`")
            })?;
            if ident == "recv" {
                let content;
                syn::parenthesized!(content in input);

                let channel: Expr = content.parse().map_err(|_| {
                    syn::Error::new(
                        ident.span(),
                        "expected a channel expression after `recv`. For example, `recv(channel)",
                    )
                })?;

                input.parse::<Token![->]>().map_err(|_| {
                    syn::Error::new(
                        channel.span(),
                        "expected `->` after channel expression.\
                         For example, `recv(channel) -> var",
                    )
                })?;
                let var: Ident = input.parse().map_err(|_| {
                    syn::Error::new(
                        channel.span(),
                        "expected a variable name after `->`.\
                         For example, `recv(channel) -> var",
                    )
                })?;

                input.parse::<Token![=>]>().map_err(|_| {
                    syn::Error::new(
                        var.span(),
                        "expected `=>` after the variable name.\
                         For example, `recv(channel) -> var => body",
                    )
                })?;
                let body: Expr = input.parse().map_err(|_| {
                    syn::Error::new(
                        var.span(),
                        "expected an expression after `=>`.\
                         For example, `recv(channel) -> var => { println!(\"received {}!\", var) }`"
                    )
                })?;

                branches.push(Branch::Recv { channel, var, body });
            } else if ident == "send" {
                let content;
                syn::parenthesized!(content in input);

                let channel: Expr = content.parse().map_err(|_| {
                    syn::Error::new(content.span(), "expected a valid channel expression inside `send(...)`")
                })?;
                content.parse::<Token![,]>()?;
                let value: Expr = content.parse().map_err(|_| {
                    syn::Error::new(content.span(), "expected a valid value expression inside `send(channel, ...)`")
                })?;

                input.parse::<Token![->]>().map_err(|_| {
                    syn::Error::new(
                        value.span(),
                        "expected `->` after value expression.\
                         For example, `send(channel, value) -> result",
                    )
                })?;
                let var: Ident = input.parse().map_err(|_| {
                    syn::Error::new(
                        value.span(),
                        "expected a variable name after `->`.\
                         For example, `send(channel, value) -> result`",
                    )
                })?;

                input.parse::<Token![=>]>().map_err(|_| {
                    syn::Error::new(
                        var.span(),
                        "expected `=>` after the variable name.\
                         For example, `send(channel, value) -> result => body",
                    )
                })?;
                let body: Expr = input.parse().map_err(|_| {
                    syn::Error::new(
                        var.span(),
                        "expected expression after `=>`.\
                         For example, `send(channel, value) -> result => {\
                          if result.is_err() { println!(\"Send operation failed!\")}`",
                    )
                })?;

                branches.push(Branch::Send {
                    channel,
                    value,
                    var,
                    body,
                });
            } else if ident == "default" {
                input.parse::<Token![=>]>().map_err(|_| {
                    syn::Error::new(
                        ident.span(),
                        "expected `=>` after `default`.\
                         For example, `default => body`",
                    )
                })?;
                let body: Expr = input.parse().map_err(|_| {
                    syn::Error::new(
                        ident.span(),
                        "expected an expression after `=>`.\
                         For example, `default => { println!(\"Nothing ready.\")` }",
                    )
                })?;

                default = Some(body);
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected `recv`, `send`, or `default`",
                ));
            }
        }

        Ok(SelectInput { branches, default })
    }
}
