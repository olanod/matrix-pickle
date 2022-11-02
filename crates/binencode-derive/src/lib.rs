// Copyright 2022 Damir Jelić
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![deny(
    clippy::mem_forget,
    clippy::unwrap_used,
    dead_code,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unsafe_op_in_unsafe_fn,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    rust_2018_idioms
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use proc_macro_crate::{crate_name, FoundCrate};
use proc_macro_error::{abort_call_site, proc_macro_error};
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, punctuated::Punctuated, token::Comma, Data, DataEnum,
    DataStruct, DeriveInput, Field, Fields, FieldsNamed, FieldsUnnamed, GenericParam, Ident, Type,
};

fn use_binencode() -> TokenStream2 {
    let found_crate = crate_name("binencode").ok().unwrap_or(FoundCrate::Itself);

    match found_crate {
        FoundCrate::Itself => quote! { crate },
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            quote! { #ident }
        }
    }
}

#[proc_macro_error]
#[proc_macro_derive(Encode)]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let binencode = use_binencode();

    for param in &mut input.generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            type_param.bounds.push(parse_quote!(#binencode::Encode));
        }
    }

    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    match input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(FieldsNamed { named, .. }),
            ..
        }) => {
            let idents = named.iter().map(|f| &f.ident);

            quote! {
                impl #impl_generics #binencode::Encode for #name #ty_generics #where_clause {
                    fn encode(&self, writer: &mut impl std::io::Write) -> Result<usize, #binencode::EncodeError> {
                        let mut ret = 0;

                        #(ret += self.#idents.encode(writer)?;)*

                        Ok(ret)
                    }
                }
            }
        }
        Data::Struct(DataStruct {
            fields: Fields::Unnamed(FieldsUnnamed { unnamed, .. }),
            ..
        }) => {
            let i = (0..unnamed.len()).map(syn::Index::from);

            quote! {
                impl #impl_generics #binencode::Encode for #name #ty_generics #where_clause {
                    fn encode(&self, writer: &mut impl std::io::Write) -> Result<usize, #binencode::EncodeError> {
                        let mut ret = 0;

                        #(ret += self.#i.encode(writer)?;)*

                        Ok(ret)
                    }
                }
            }
        }
        Data::Enum(DataEnum { variants, .. }) => {
            let names = variants.iter().map(|v| &v.ident);
            let numbers = 0u8..variants.len().try_into().expect("Only enums with up to 256 elements are supported");

            quote! {
                impl #impl_generics #binencode::Encode for #name #ty_generics #where_clause {
                    fn encode(&self, writer: &mut impl std::io::Write) -> Result<usize, #binencode::EncodeError> {
                        let mut ret = 0;

                        match self {
                            #(#name::#names(v) => {
                                ret += #numbers.encode(writer)?;
                                ret += v.encode(writer)?;
                            }),*
                        }

                        Ok(ret)
                    }
                }
            }
        }

        _ => abort_call_site!("`#[derive(Encode)` only supports non-tuple structs"),
    }.into()
}

fn check_if_boxed(fields: &Punctuated<Field, Comma>) {
    for field in fields {
        for attribute in &field.attrs {
            if attribute.path.is_ident("secret") {
                match &field.ty {
                    Type::Array(_) => abort_call_site!(
                        "Arrays need to be boxed to avoid unintended copies of the secret"
                    ),
                    Type::Path(_) => {}
                    _ => {
                        abort_call_site!("Type {} does not support being decoded as a secret value")
                    }
                }
            }
        }
    }
}

#[proc_macro_error]
#[proc_macro_derive(Decode, attributes(secret))]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let binencode = use_binencode();

    for param in &mut input.generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            type_param.bounds.push(parse_quote!(#binencode::Encode));
        }
    }

    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    match input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(FieldsNamed { named, .. }),
            ..
        }) => {
            check_if_boxed(&named);

            let names = named.iter().map(|f| &f.ident);
            let field_types = named.iter().map(|f| &f.ty);

            quote! {
                impl #impl_generics #binencode::Decode for #name #ty_generics #where_clause {
                    fn decode(reader: &mut impl std::io::Read) -> Result<Self, #binencode::DecodeError> {
                        Ok(Self {
                            #(#names: <#field_types>::decode(reader)?),*
                        })
                    }
                }
            }
        }
        Data::Struct(DataStruct {
            fields: Fields::Unnamed(FieldsUnnamed { unnamed, .. }),
            ..
        }) => {
            check_if_boxed(&unnamed);

            let field_types = unnamed.iter().map(|f| &f.ty);

            quote! {
                impl #impl_generics #binencode::Decode for #name #ty_generics #where_clause {
                    fn decode(reader: &mut impl std::io::Read) -> Result<Self, #binencode::DecodeError> {
                        Ok(Self (
                            #(<#field_types>::decode(reader)?),*
                        ))
                    }
                }
            }
        }
        Data::Enum(DataEnum { variants, .. }) => {
            let names = variants.iter().map(|v| &v.ident);
            let numbers = 0u8..variants.len().try_into().expect("Only enums with up to 256 elements are supported");

            quote! {
                impl #impl_generics #binencode::Decode for #name #ty_generics #where_clause {
                    fn decode(reader: &mut impl std::io::Read) -> Result<Self, #binencode::DecodeError> {
                        let variant = u8::decode(reader)?;

                        match variant {
                            #(#numbers => {
                                let x = Decode::decode(reader)?;
                                Ok(Self::#names(x))
                            }),*

                            _ => Err(#binencode::DecodeError::UnknownEnumVariant(variant))
                        }
                    }
                }
            }
        }
        _ => abort_call_site!("`#[derive(Encode)` only supports non-tuple structs"),
    }.into()
}
