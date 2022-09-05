//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, parse_quote, Expr, ExprCall, ExprMethodCall, Local, Path, Result, Stmt, punctuated::Punctuated, token::Comma};

use self::{ast::TransactionAst, builder::VariableIdent};

mod ast;
pub mod builder;

pub fn generate_transaction(input: TokenStream) -> Result<TokenStream> {
    let ast = parse2::<TransactionAst>(input).unwrap();
    let input_stmts = ast.stmts;

    let mut output_stmts: Vec<Stmt> = vec![];
    output_stmts.push(parse_quote! {
        let mut builder = TransactionBuilder::new();
    });

    for stmt in input_stmts {
        let mut instruction_stmts: Vec<Stmt> = match stmt {
            syn::Stmt::Local(binding) => instruction_from_local(binding),
            syn::Stmt::Expr(expr) => instruction_from_expr(expr),
            syn::Stmt::Semi(expr, _) => instruction_from_expr(expr),
            syn::Stmt::Item(_) => todo!(),
        };

        output_stmts.append(&mut instruction_stmts);
    }

    // we return the transaction builder
    output_stmts.push(parse_quote! {
        return builder;
    });

    let output_tokens = quote! {
        {
            #(#output_stmts)*
        }
    };

    Ok(output_tokens)
}

pub fn instruction_from_local(local: Local) -> Vec<Stmt> {
    let variable_name = match local.pat {
        syn::Pat::Ident(ident) => ident.ident.to_string(),
        syn::Pat::Type(pt) => {
            match *pt.pat {
                syn::Pat::Ident(ident) => ident.ident.to_string(),
                _ => todo!(),
            }

            // TODO: handle mutability? types?
        },
        syn::Pat::Tuple(tuple) => {
            // TODO: properly handle all elements in the tuple
            match &tuple.elems[0] {
                syn::Pat::Ident(ident) => ident.ident.to_string(),
                _ => todo!(),
            }
        },
        _ => {
            println!("TODO: {:#?}", local.pat);
            todo!()
        },
    };

    let (_, expr) = local.init.unwrap();
    let result = match *expr {
        Expr::Call(call) => build_call_function(call, variable_name),
        Expr::MethodCall(call) => Some(build_call_method(call)),
        _ => {
            println!("TODO: {:#?}", *expr);
            todo!()
        },
    };

    // TODO: handle literal assigments ("let x = Amount(1_000)")
    if let Some(stmt) = result {
        return vec![stmt];
    }

    vec![]
}

fn build_call_function(expr: ExprCall, variable_name: VariableIdent) -> Option<Stmt> {
    let segments = match *expr.func {
        Expr::Path(expr_path) => path_segments(expr_path.path),
        _ => todo!(),
    };

    // "Struct::function"
    if segments.len() != 2 {
        return None;
    }

    let template = segments[0].clone();
    let function = segments[1].clone();

    let arg_exprs = build_call_args(expr.args);

    let expr = parse_quote! {
        builder.add_instruction(Instruction::CallFunction {
            package_address: String::new(),
            template: #template,
            function: #function,
            proofs: vec![],
            args: vec![#(#arg_exprs),*],
            return_variables: vec![#variable_name],
        });
    };

    Some(expr)
}

fn path_segments(path: Path) -> Vec<String> {
    path.segments.iter().map(|s| s.ident.to_string()).collect()
}

pub fn instruction_from_expr(expr: Expr) -> Vec<Stmt> {
    let stmt = match expr {
        Expr::Assign(_) => todo!(),
        Expr::Call(_) => todo!(),
        Expr::Field(_) => todo!(),
        Expr::Lit(_) => todo!(),
        Expr::MethodCall(call) => build_call_method(call),
        Expr::Tuple(tuple_expr) => {
            println!("TODO: {:#?}", tuple_expr);
            todo!()
        },
        _ => todo!(),
    };

    vec![stmt]
}

fn build_call_method(expr: ExprMethodCall) -> Stmt {
    let method = expr.method.to_string();

    // TODO: component address, etc

    let arg_exprs = build_call_args(expr.args);

    parse_quote! {
        builder.add_instruction(Instruction::CallMethod {
            package_address: String::new(),
            component_address: String::new(),
            method: #method,
            proofs: vec![],
            args: vec![#(#arg_exprs),*],
            return_variables: vec![],
        });
    }
}

fn build_call_args(args: Punctuated<Expr, Comma>) -> Vec<Expr> {
    args.iter().map(|arg| {
        match arg {
            Expr::Lit(lit) => {
                parse_quote! { #lit }
            },
            Expr::Path(expr_path) => {
                // variable names should only have one segment
                let variable_name = &expr_path.path.segments[0].ident.to_string();
                parse_quote! { Variable(#variable_name) }
            },
            _ => todo!(),
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use indoc::indoc;
    use proc_macro2::TokenStream;
    use quote::quote;

    use super::generate_transaction;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn check_correct_code_generation() {
        let input = TokenStream::from_str(indoc! {"
            // initialize the component
            let mut picture_seller = PictureSeller::new(1_000);

            // initialize a user account with enough funds
            let mut account = Account::new();
            let funds = ThaumFaucet::take(1_000);
            account.add_fungible(funds);

            // buy a picture
            let payment: Bucket<Thaum> = account.take_fungible(1_000);
            let (picture, _) = picture_seller.buy(payment);

            // store our brand new picture in our account
            account.add_non_fungible(picture);
        "})
        .unwrap();

        let output = generate_transaction(input).unwrap();

        assert_code_eq(output, quote! {
            {
                let mut builder = TransactionBuilder::new();
                builder.add_instruction(Instruction::CallFunction {
                    package_address: String::new(),
                    template: "PictureSeller",
                    function: "new",
                    proofs: vec![],
                    args: vec![1_000],
                    return_variables: vec!["picture_seller"],
                });
                builder.add_instruction(Instruction::CallFunction {
                    package_address : String::new(),
                    template: "Account",
                    function: "new",
                    proofs: vec![],
                    args: vec![],
                    return_variables: vec!["account"],
                });
                builder.add_instruction(Instruction :: CallFunction {
                    package_address: String::new(),
                    template: "ThaumFaucet",
                    function: "take",
                    proofs: vec![],
                    args: vec![1_000],
                    return_variables: vec!["funds"],
                });
                builder.add_instruction(Instruction::CallMethod {
                    package_address: String::new(),
                    component_address: String::new(),
                    method: "add_fungible",
                    proofs: vec![],
                    args: vec![Variable("funds")],
                    return_variables: vec![],
                });
                builder.add_instruction(Instruction::CallMethod {
                    package_address: String::new(),
                    component_address: String::new(),
                    method: "take_fungible",
                    proofs: vec![],
                    args: vec![1_000],
                    return_variables: vec![],
                });
                builder.add_instruction(Instruction::CallMethod {
                    package_address: String::new(),
                    component_address: String::new(),
                    method: "buy",
                    proofs: vec![],
                    args: vec![Variable("payment")],
                    return_variables: vec![],
                });
                builder.add_instruction(Instruction::CallMethod {
                    package_address: String::new(),
                    component_address: String::new(),
                    method: "add_non_fungible",
                    proofs: vec![],
                    args: vec![Variable("picture")],
                    return_variables: vec![],
                });
                return builder;
            }
        });
    }

    #[allow(dead_code)]
    fn assert_code_eq(a: TokenStream, b: TokenStream) {
        assert_eq!(a.to_string(), b.to_string());
    }
}
