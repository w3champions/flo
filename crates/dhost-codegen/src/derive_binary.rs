use darling::error::Error;
use darling::{ast, FromDeriveInput, FromField, FromMeta};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(bin), supports(struct_named))]
pub struct DecodeInputReceiver {
  ident: syn::Ident,
  generics: syn::Generics,
  data: ast::Data<(), FieldReceiver>,
  #[darling(default)]
  mod_path: Option<syn::Path>,
}

impl DecodeInputReceiver {
  fn gen_min_size(
    &self,
    mod_path: &syn::Path,
    FieldReceiver {
      ref ty,
      ref bitflags,
      ..
    }: &FieldReceiver,
  ) -> TokenStream {
    match *ty {
      syn::Type::Array(syn::TypeArray {
        ref elem, ref len, ..
      }) => quote! {
        (<#elem as #mod_path::BinDecode>::MIN_SIZE * (#len))
      },
      _ => {
        if let Some(repr) = bitflags {
          quote! {
            <#repr as #mod_path::BinDecode>::MIN_SIZE
          }
        } else {
          quote! {
            <#ty as #mod_path::BinDecode>::MIN_SIZE
          }
        }
      }
    }
  }

  fn gen_fixed_size(
    &self,
    mod_path: &syn::Path,
    FieldReceiver {
      ref ty,
      ref bitflags,
      ..
    }: &FieldReceiver,
  ) -> TokenStream {
    match *ty {
      syn::Type::Array(syn::TypeArray { ref elem, .. }) => quote! {
        <#elem as #mod_path::BinDecode>::FIXED_SIZE
      },
      _ => {
        if bitflags.is_some() {
          quote! { true }
        } else {
          quote! {
            <#ty as #mod_path::BinDecode>::FIXED_SIZE
          }
        }
      }
    }
  }

  fn gen_decode(&self, mod_path: &syn::Path, field: &FieldReceiver) -> TokenStream {
    let FieldReceiver {
      ref ident,
      ref ty,
      bitflags: ref bitflags_repr,
      ..
    } = field;
    match ty {
      syn::Type::Array(syn::TypeArray {
        ref elem, ref len, ..
      }) => {
        if let syn::Expr::Lit(syn::ExprLit {
          lit: syn::Lit::Int(ref lit_int),
          ..
        }) = *len
        {
          if let Ok(size) = lit_int.base10_parse::<usize>() {
            if is_type_u8(elem) {
              quote! {
                {
                  let mut arr = [0; #size];
                  buf.copy_to_slice(&mut arr as &mut [u8]);
                  arr
                }
              }
            } else {
              let items: Punctuated<TokenStream, syn::Token![,]> = std::iter::repeat(quote! {
                <#elem as #mod_path::BinDecode>::decode(buf)?
              })
              .take(size)
              .collect();
              quote! {
                [#items]
              }
            }
          } else {
            quote::quote_spanned! {
              len.span() => compile_error!("array size must be a literal int");
            }
          }
        } else {
          quote::quote_spanned! {
            len.span() => compile_error!("array size must be a literal int");
          }
        }
      }
      _ => {
        if let Some(bitflags_repr) = bitflags_repr.as_ref() {
          let ident_str = ident.as_ref().unwrap().to_string();
          quote! {
            {
              let bits = <#bitflags_repr as #mod_path::BinDecode>::decode(buf)?;
              #ty::from_bits(bits)
                .ok_or_else(|| #mod_path::BinDecodeError::failure(
                  format!("Unexpected flag value for field `{}`: 0x{:x}",
                    #ident_str,
                    bits
                  )
                ))?
            }
          }
        } else {
          quote! {
            <#ty as #mod_path::BinDecode>::decode(buf)?
          }
        }
      }
    }
  }
}

impl ToTokens for DecodeInputReceiver {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let DecodeInputReceiver {
      ref ident,
      ref generics,
      ref data,
      ref mod_path,
    } = *self;

    let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
      dhost_util::binary
    });
    let fields = data.as_ref().take_struct().unwrap();
    let field_idents_list_comma: Punctuated<&syn::Ident, syn::Token![,]> =
      fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();

    let min_size_plus_list: Punctuated<TokenStream, syn::Token![+]> = fields
      .iter()
      .map(|f| self.gen_min_size(&mod_path, &f))
      .collect();

    let mut decode_field_items = Vec::with_capacity(fields.len());
    for (i, field) in fields.iter().enumerate() {
      let ident = &field.ident;
      let check_remaining = if i > 0 && i < fields.len() {
        let items: Punctuated<&TokenStream, syn::Token![+]> =
          min_size_plus_list.iter().skip(i).collect();
        let last_ty_fixed_size = self.gen_fixed_size(&mod_path, &fields.fields[i - 1]);
        // check remaining after decoding every non-fixed sized field
        quote! {
          if !#last_ty_fixed_size {
            if buf.remaining() < #items {
              return Err(#mod_path::BinDecodeError::Incomplete);
            }
          }
        }
      } else {
        quote! {}
      };
      let check_value = if let Some(Eq { ref value }) = &field.eq {
        let ident_str = ident.as_ref().unwrap().to_string();
        quote! {
          if #ident != #value {
            return Err(#mod_path::BinDecodeError::failure(
              format!("Unexpected value for field `{}`, expected `{:?}`, got `{:?}`",
                #ident_str,
                #ident,
                #value
              )
            ));
          }
        }
      } else {
        quote! {}
      };
      let decode = self.gen_decode(&mod_path, &field);
      decode_field_items.push(quote! {
        let #ident = {
          #check_remaining
          #decode
        };

        #check_value
      });
    }

    let fixed_size_and_list: Punctuated<TokenStream, syn::Token![&&]> = fields
      .iter()
      .map(|f| self.gen_fixed_size(&mod_path, &f))
      .collect();

    let (imp, ty, wher) = generics.split_for_impl();

    tokens.extend(quote! {
      impl #imp #mod_path::BinDecode for #ident #ty #wher {
        const MIN_SIZE: usize = #min_size_plus_list;
        const FIXED_SIZE: bool = #fixed_size_and_list;
        fn decode<T: #mod_path::Buf>(buf: &mut T) -> Result<Self, #mod_path::BinDecodeError> {
          if buf.remaining() < Self::MIN_SIZE {
            return Err(#mod_path::BinDecodeError::Incomplete);
          }

          #(#decode_field_items)*

          Ok(Self {
            #field_idents_list_comma
          })
        }
      }
    })
  }
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(bin), supports(struct_named))]
pub struct EncodeInputReceiver {
  ident: syn::Ident,
  generics: syn::Generics,
  data: ast::Data<(), FieldReceiver>,
  #[darling(default)]
  mod_path: Option<syn::Path>,
}

impl EncodeInputReceiver {
  fn gen_encode(&self, mod_path: &syn::Path, field: &FieldReceiver) -> TokenStream {
    let FieldReceiver {
      ref ident,
      ref ty,
      bitflags: ref bitflags_repr,
      ..
    } = field;
    match *ty {
      syn::Type::Array(syn::TypeArray {
        ref elem, ref len, ..
      }) => {
        if let syn::Expr::Lit(syn::ExprLit {
          lit: syn::Lit::Int(ref lit_int),
          ..
        }) = *len
        {
          if let Ok(size) = lit_int.base10_parse::<usize>() {
            if is_type_u8(elem) {
              quote! {
                buf.put_slice(&self.#ident as &[u8]);
              }
            } else {
              let mut items = Vec::with_capacity(size);
              for i in 0..size {
                items.push(quote! {
                  <#elem as #mod_path::BinEncode>::encode(&self.#ident[#i], buf);
                })
              }
              quote! {
                #(#items)*
              }
            }
          } else {
            quote::quote_spanned! {
              len.span() => compile_error!("array size must be a literal int");
            }
          }
        } else {
          quote::quote_spanned! {
            len.span() => compile_error!("array size must be a literal int");
          }
        }
      }
      _ => {
        if let Some(bitflags_repr) = bitflags_repr.as_ref() {
          quote! {
            <#bitflags_repr as #mod_path::BinEncode>::encode(&self.#ident.bits(), buf);
          }
        } else {
          quote! {
            <#ty as #mod_path::BinEncode>::encode(&self.#ident, buf);
          }
        }
      }
    }
  }
}

impl ToTokens for EncodeInputReceiver {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let EncodeInputReceiver {
      ref ident,
      ref generics,
      ref data,
      ref mod_path,
    } = *self;

    let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
      dhost_util::binary
    });
    let fields = data.as_ref().take_struct().unwrap();

    let mut field_items = Vec::with_capacity(fields.len());
    for field in fields.iter() {
      let encode = self.gen_encode(&mod_path, &field);
      field_items.push(quote! {
        #encode
      });
    }

    let (imp, ty, wher) = generics.split_for_impl();

    tokens.extend(quote! {
      impl #imp #mod_path::BinEncode for #ident #ty #wher {
        fn encode<T: #mod_path::BufMut>(&self, buf: &mut T) {
          #(#field_items)*
        }
      }
    })
  }
}

#[derive(Debug, FromField)]
#[darling(attributes(bin))]
struct FieldReceiver {
  ident: Option<syn::Ident>,
  ty: syn::Type,
  #[darling(default)]
  eq: Option<Eq>,
  #[darling(default)]
  bitflags: Option<syn::Path>,
}

// eq(0x0)
#[derive(Debug)]
struct Eq {
  value: syn::Lit,
}

impl FromMeta for Eq {
  fn from_value(item: &syn::Lit) -> Result<Self, Error> {
    Ok(Self {
      value: item.clone(),
    })
  }
}

fn is_type_u8(ty: &syn::Type) -> bool {
  if let syn::Type::Path(syn::TypePath { ref path, .. }) = *ty {
    if let Some(t) = path.get_ident().map(|i| i.to_string()) {
      t == "u8"
    } else {
      false
    }
  } else {
    false
  }
}
