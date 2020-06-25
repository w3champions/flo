use darling::error::Error;
use darling::{ast, FromDeriveInput, FromField, FromMeta, FromVariant};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

//TODO: check enum value order for correct PartialOrd behavior

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(bin), supports(struct_named, enum_unit))]
pub struct DecodeInputReceiver {
  ident: syn::Ident,
  generics: syn::Generics,
  data: ast::Data<VariantReceiver, FieldReceiver>,
  #[darling(default)]
  mod_path: Option<syn::Path>,
  #[darling(default)]
  enum_repr: Option<EnumRepr>,
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
    if get_option_type_arg(ty).is_some() {
      return quote! { 0 };
    }

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
    if get_option_type_arg(ty).is_some() {
      return quote! { false };
    }

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

  fn gen_decode_as_ty(
    &self,
    mod_path: &syn::Path,
    field: &FieldReceiver,
    ty: &syn::Type,
  ) -> TokenStream {
    let FieldReceiver {
      ref ident,
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
            if is_type_likely_u8(elem) {
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
                  format!("unexpected flag value for field `{}`: 0x{:x}",
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

  fn gen_decode(&self, mod_path: &syn::Path, field: &FieldReceiver) -> TokenStream {
    let FieldReceiver {
      ref ident,
      ref ty,
      ref condition,
      ..
    } = field;
    if let Some(opt_ty) = get_option_type_arg(ty) {
      let expr = if let Some(FieldCondition { ref expr }) = condition {
        quote! { #expr }
      } else {
        return quote::quote_spanned! {
          ident.span() => compile_error!("#[bin(condition = \"expr\"] attribute is required to decode an Option<T>");
        };
      };
      let decode = self.gen_decode_as_ty(mod_path, field, &opt_ty);
      return quote! {
        if #expr {
          Some(#decode)
        } else {
          None
        }
      };
    } else {
      self.gen_decode_as_ty(mod_path, field, ty)
    }
  }

  fn to_struct_tokens(&self, tokens: &mut TokenStream) {
    let DecodeInputReceiver {
      ref ident,
      ref generics,
      ref data,
      ref mod_path,
      ..
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
              return Err(#mod_path::BinDecodeError::incomplete());
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
              format!("unexpected value for field `{}`, expected `{:?}`, got `{:?}`",
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
            return Err(#mod_path::BinDecodeError::incomplete());
          }

          #(#decode_field_items)*

          Ok(Self {
            #field_idents_list_comma
          })
        }
      }
    })
  }

  fn to_enum_tokens(&self, tokens: &mut TokenStream) {
    let DecodeInputReceiver {
      ref ident,
      ref generics,
      ref data,
      ref mod_path,
      ref enum_repr,
    } = *self;

    let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
      dhost_util::binary
    });
    let variants = data.as_ref().take_enum().unwrap();
    let repr = enum_repr.as_ref().map(|v| v.ty.clone()).unwrap_or_else(|| {
      syn::parse_quote! { u32 }
    });

    // (value => Enum::Variant),*
    let known_items: Punctuated<TokenStream, syn::Token![,]> = variants
      .iter()
      .map(|v| {
        let value = &v.value;
        let vident = &v.ident;
        quote::quote_spanned! {
          v.ident.span() => #value => Ok(#ident::#vident)
        }
      })
      .collect();

    let (imp, ty, wher) = generics.split_for_impl();

    tokens.extend(quote! {
      impl #imp #mod_path::BinDecode for #ident #ty #wher {
        const MIN_SIZE: usize = <#repr as #mod_path::BinDecode>::MIN_SIZE;
        const FIXED_SIZE: bool = <#repr as #mod_path::BinDecode>::FIXED_SIZE;
        fn decode<T: #mod_path::Buf>(buf: &mut T) -> Result<Self, #mod_path::BinDecodeError> {
          if buf.remaining() < Self::MIN_SIZE {
            return Err(#mod_path::BinDecodeError::incomplete());
          }

          match <#repr as #mod_path::BinDecode>::decode(buf)? {
            #known_items,
            __v => Err(#mod_path::BinDecodeError::failure(format!(
              "unknown value for enum type `{}`: {:?}", stringify!(#ident), __v
            )))
          }
        }
      }
    })
  }
}

impl ToTokens for DecodeInputReceiver {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    match self.data {
      ast::Data::Enum(_) => self.to_enum_tokens(tokens),
      ast::Data::Struct(_) => self.to_struct_tokens(tokens),
    }
  }
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(bin), supports(struct_named, enum_unit))]
pub struct EncodeInputReceiver {
  ident: syn::Ident,
  generics: syn::Generics,
  data: ast::Data<VariantReceiver, FieldReceiver>,
  #[darling(default)]
  mod_path: Option<syn::Path>,
  #[darling(default)]
  enum_repr: Option<EnumRepr>,
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
            if is_type_likely_u8(elem) {
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
      ref enum_repr,
    } = *self;

    let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
      dhost_util::binary
    });

    let (imp, ty, wher) = generics.split_for_impl();

    match data {
      ast::Data::Struct(fields) => {
        let mut field_items = Vec::with_capacity(fields.len());
        for field in fields.iter() {
          let encode = self.gen_encode(&mod_path, &field);
          field_items.push(quote! {
            #encode
          });
        }

        tokens.extend(quote! {
          impl #imp #mod_path::BinEncode for #ident #ty #wher {
            fn encode<T: #mod_path::BufMut>(&self, buf: &mut T) {
              #(#field_items)*
            }
          }
        })
      }
      ast::Data::Enum(variants) => {
        let repr = enum_repr.as_ref().map(|v| v.ty.clone()).unwrap_or_else(|| {
          syn::parse_quote! { u32 }
        });

        // (Enum::Variant => value),*
        let items: Punctuated<TokenStream, syn::Token![,]> = variants
          .iter()
          .map(|v| {
            let value = &v.value;
            let vident = &v.ident;
            quote::quote_spanned! {
              v.ident.span() => #ident::#vident => #value
            }
          })
          .collect();

        tokens.extend(quote! {
          impl #imp #mod_path::BinEncode for #ident #ty #wher {
            fn encode<T: #mod_path::BufMut>(&self, buf: &mut T) {
              <#repr as #mod_path::BinEncode>::encode(&self.bin_repr_value(), buf);
            }
          }

          impl #imp #ident #ty #wher {
            fn bin_repr_value(&self) -> #repr {
              match *self {
                #items
              }
            }
          }
        })
      }
    }
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
  #[darling(default)]
  condition: Option<FieldCondition>,
}

#[derive(Debug, FromVariant)]
#[darling(attributes(bin))]
struct VariantReceiver {
  ident: syn::Ident,
  value: syn::Lit,
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

#[derive(Debug)]
struct EnumRepr {
  ty: syn::Path,
}

impl FromMeta for EnumRepr {
  fn from_meta(item: &syn::Meta) -> Result<Self, Error> {
    if let syn::Meta::List(syn::MetaList { ref nested, .. }) = *item {
      if nested.len() == 1 {
        if let Some(syn::NestedMeta::Meta(syn::Meta::Path(ref path))) = nested.first() {
          if path.get_ident().is_some() {
            return Ok(EnumRepr { ty: path.clone() });
          }
        }
      }
    }
    Err(Error::custom("invalid syntax, `#[bin(enum_repr(Type))]` expected").with_span(item))
  }
}

#[derive(Debug)]
struct FieldCondition {
  expr: syn::Expr,
}

impl FromMeta for FieldCondition {
  fn from_string(value: &str) -> Result<Self, Error> {
    let expr =
      syn::parse_str(value).map_err(|e| Error::custom(format!("invalid syntax: {}", e)))?;
    Ok(FieldCondition { expr })
  }
}

fn is_type_likely_u8(ty: &syn::Type) -> bool {
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

fn get_option_type_arg(ty: &syn::Type) -> Option<syn::Type> {
  if let syn::Type::Path(syn::TypePath {
    qself: None,
    ref path,
    ..
  }) = *ty
  {
    if path.leading_colon.is_none() {
      if path.segments.len() == 1 {
        if let Some(syn::PathSegment {
          ref ident,
          arguments:
            syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
              colon2_token: None,
              ref args,
              ..
            }),
          ..
        }) = path.segments.first()
        {
          if args.len() == 1 {
            if ident.to_string() == "Option" {
              if let Some(syn::GenericArgument::Type(ref ty)) = args.first() {
                return Some(ty.clone());
              }
            }
          }
        }
      }
    }
  }
  None
}
