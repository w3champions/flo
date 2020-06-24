extern crate proc_macro;
use darling::FromDeriveInput;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod derive_binary;

#[proc_macro_derive(BinDecode, attributes(bin))]
pub fn derive_bin_decode(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  let receiver = derive_binary::DecodeInputReceiver::from_derive_input(&input).unwrap();
  TokenStream::from(quote!(#receiver))
}

#[proc_macro_derive(BinEncode, attributes(bin))]
pub fn derive_bin_encode(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  let receiver = derive_binary::EncodeInputReceiver::from_derive_input(&input).unwrap();
  TokenStream::from(quote!(#receiver))
}
