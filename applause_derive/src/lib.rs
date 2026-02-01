// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use heck::ToKebabCase;
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Implement ArgsToVec on an Args struct.
///
/// ArgsToVec::args_to_vec tries to reproduce a vector of arguments which would be interpreted by
/// clap in such a way as to reproduce the Args struct this is derived from. There are plenty of
/// limitations on this, many which I probably haven't even conceived of, but at the very least:
///
/// * Each field of the Args struct must be an `#[arg(...)]`
/// * Each arg must have a default `long` attribute
/// * Each arg must be of type `Option<T> where T: AsRef<OsStr>`
#[proc_macro_derive(ArgsToVec)]
pub fn derive_args_to_vec(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let vec = data_to_vec(&input.data);

    let expanded = quote! {
        impl #impl_generics applause::ArgsToVec for #name #ty_generics #where_clause {
            fn args_to_vec(&self) -> Vec<OsString> {
                let mut v = vec![];
                #vec
                v
            }
        }
    };

    proc_macro::TokenStream::from(expanded)
}

fn data_to_vec(data: &Data) -> TokenStream {
    let mut pushes = vec![];
    match *data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                for field in fields.named.iter() {
                    let span = field.span();
                    let field_name = field.ident.as_ref().unwrap();
                    let s = field_name.unraw().to_string();
                    let arg_name = s.to_kebab_case();
                    let flag = format!("--{arg_name}=");
                    pushes.push(quote_spanned!(span=> {
                        if let Some(ref x) = self.#field_name {
                            let mut arg = OsString::new();
                            arg.push(#flag);
                            arg.push(x);
                            v.push(arg);
                        }
                    }));
                }
            }
            Fields::Unnamed(_) | Fields::Unit => unimplemented!(),
        },
        Data::Enum(_) | Data::Union(_) => unimplemented!(),
    };
    quote! {{
        #( #pushes )*
    }}
}
