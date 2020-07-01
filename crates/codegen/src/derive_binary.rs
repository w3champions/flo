use darling::error::Error;
use darling::{ast, FromDeriveInput, FromField, FromMeta, FromVariant};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

//TODO: check enum value order for correct PartialOrd behavior

#[derive(Debug, FromDeriveInput)]
#[darling(
  attributes(bin),
  supports(struct_named, struct_unit, struct_newtype, enum_unit, enum_newtype)
)]
pub struct DecodeInputReceiver {
  ident: syn::Ident,
  generics: syn::Generics,
  data: ast::Data<VariantReceiver, FieldReceiver>,
  #[darling(default)]
  mod_path: Option<syn::Path>,
  #[darling(default)]
  enum_repr: Option<MetaType>,
}

impl DecodeInputReceiver {
  fn gen_min_size(
    &self,
    mod_path: &syn::Path,
    ty: &syn::Type,
    repeat: Option<&MetaExpr>,
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
        if repeat.is_some() {
          quote! {0}
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
      ref repeat,
      ref bitflags,
      ..
    }: &FieldReceiver,
  ) -> TokenStream {
    if get_option_type_arg(ty).is_some() {
      return quote! { false };
    }

    if bitflags.is_some() {
      return quote! { true };
    }

    match *ty {
      syn::Type::Array(syn::TypeArray { ref elem, .. }) => quote! {
        <#elem as #mod_path::BinDecode>::FIXED_SIZE
      },
      _ => {
        if repeat.is_some() {
          quote! { false }
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
      ref repeat,
      bitflags,
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
        if let Some(ref repeat) = repeat.as_ref() {
          let len = &repeat.expr;
          quote! {
            #mod_path::BinBufExt::get_repeated(buf, (#len) as usize)?
          }
        } else {
          if let Some(bitflags) = bitflags.as_ref() {
            let repr_type = &bitflags.ty;
            quote! {
              {
                buf.check_size(<#repr_type as #mod_path::BinDecode>::MIN_SIZE)?;
                let bits = <#repr_type as #mod_path::BinDecode>::decode(buf)?;
                if let Some(flags) = #ty::from_bits(bits) {
                  flags
                } else {
                  return Err(#mod_path::BinDecodeError::failure(
                    format!("representation contains bits that do not correspond to the flag type `{}`: {:b}",
                      stringify!(#ty),
                      !#ty::all().bits() & bits,
                    )
                  ));
                }
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

  fn gen_decode(&self, mod_path: &syn::Path, field: &FieldReceiver) -> TokenStream {
    let FieldReceiver {
      ref ident,
      ref ty,
      ref condition,
      ref repeat,
      ..
    } = field;
    if let Some(opt_ty) = get_option_type_arg(ty) {
      let expr = if let Some(MetaExpr { ref expr }) = condition {
        quote! { #expr }
      } else {
        return quote::quote_spanned! {
          ident.span() => compile_error!("#[bin(condition = \"expr\"] attribute is required to decode an Option<T>");
        };
      };
      let decode = self.gen_decode_as_ty(mod_path, field, &opt_ty);
      let opt_min_size = self.gen_min_size(mod_path, &opt_ty, repeat.as_ref());
      return quote! {
        if #expr {
          if #opt_min_size > 0 && buf.remaining()  < #opt_min_size {
            return Err(#mod_path::BinDecodeError::incomplete());
          }
          Some(#decode)
        } else {
          None
        }
      };
    } else {
      self.gen_decode_as_ty(mod_path, field, ty)
    }
  }

  fn to_struct_newtype_tokens(&self, tokens: &mut TokenStream) {
    let DecodeInputReceiver {
      ref ident,
      ref generics,
      ref data,
      ref mod_path,
      ..
    } = *self;
    let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
      flo_util::binary
    });
    let repr_ty = &data
      .as_ref()
      .take_struct()
      .unwrap()
      .fields
      .first()
      .unwrap()
      .ty;
    let (imp, ty, wher) = generics.split_for_impl();
    tokens.extend(quote! {
      impl #imp #mod_path::BinDecode for #ident #ty #wher {
        const MIN_SIZE: usize = <#repr_ty as #mod_path::BinDecode>::MIN_SIZE;
        const FIXED_SIZE: bool = <#repr_ty as #mod_path::BinDecode>::FIXED_SIZE;
        fn decode<T: #mod_path::Buf>(buf: &mut T) -> Result<Self, #mod_path::BinDecodeError> {
          Ok(Self(<#repr_ty as #mod_path::BinDecode>::decode(buf)?))
        }
      }
    });
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
      flo_util::binary
    });
    let fields = data.as_ref().take_struct().unwrap();

    let field_idents_list_comma: Punctuated<&syn::Ident, syn::Token![,]> =
      fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();

    let min_size_plus_list: Punctuated<TokenStream, syn::Token![+]> = fields
      .iter()
      .map(|f| {
        let ty = if let Some(MetaType { ref ty }) = f.bitflags {
          ty
        } else {
          &f.ty
        };
        self.gen_min_size(&mod_path, ty, f.repeat.as_ref())
      })
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
          if #items > 0 && !#last_ty_fixed_size {
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
                #value,
                #ident
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
      ..
    } = *self;

    let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
      flo_util::binary
    });
    let variants = data.as_ref().take_enum().unwrap();
    let repr = enum_repr.as_ref().map(|v| v.ty.clone()).unwrap_or_else(|| {
      syn::parse_quote! { u32 }
    });

    // (value => Enum::Variant),*
    let known_items: Punctuated<TokenStream, syn::Token![,]> = variants
      .iter()
      .filter(|v| v.ident.to_string() != "UnknownValue")
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
            v => Ok(#ident::UnknownValue(v)),
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
      ast::Data::Struct(ref fields) => {
        if fields.is_newtype() {
          self.to_struct_newtype_tokens(tokens)
        } else if fields.is_unit() {
          let DecodeInputReceiver {
            ref ident,
            ref mod_path,
            ..
          } = *self;
          let mod_path: syn::Path = mod_path.clone().unwrap_or(syn::parse_quote! {
            flo_util::binary
          });

          tokens.extend(quote! {
            impl #mod_path::BinDecode for #ident {
              const MIN_SIZE: usize = 0;
              const FIXED_SIZE: bool = true;
              fn decode<T: #mod_path::Buf>(buf: &mut T) -> Result<Self, #mod_path::BinDecodeError> {
                Ok(#ident)
              }
            }
          })
        } else {
          self.to_struct_tokens(tokens)
        }
      }
    }
  }
}

#[derive(Debug, FromDeriveInput)]
#[darling(
  attributes(bin),
  supports(struct_named, struct_unit, struct_newtype, enum_unit, enum_newtype)
)]
pub struct EncodeInputReceiver {
  ident: syn::Ident,
  generics: syn::Generics,
  data: ast::Data<VariantReceiver, FieldReceiver>,
  #[darling(default)]
  mod_path: Option<syn::Path>,
  #[darling(default)]
  enum_repr: Option<MetaType>,
}

impl EncodeInputReceiver {
  fn gen_encode_as_ty(
    &self,
    mod_path: &syn::Path,
    accessor: TokenStream,
    ty: &syn::Type,
  ) -> TokenStream {
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
                buf.put_slice(#accessor as &[u8]);
              }
            } else {
              let mut items = Vec::with_capacity(size);
              for i in 0..size {
                items.push(quote! {
                  <#elem as #mod_path::BinEncode>::encode(#accessor[#i], buf);
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
        quote! {
          <#ty as #mod_path::BinEncode>::encode(#accessor, buf);
        }
      }
    }
  }

  fn gen_encode(&self, mod_path: &syn::Path, field: &FieldReceiver) -> TokenStream {
    let FieldReceiver {
      ref ident,
      ref ty,
      ref condition,
      bitflags,
      ..
    } = field;
    if let Some(opt_ty) = get_option_type_arg(ty) {
      let expr = if let Some(MetaExpr { ref expr }) = condition {
        let expr = add_self_to_idents(expr);
        quote! { #expr }
      } else {
        return quote::quote_spanned! {
          ident.span() => compile_error!("#[bin(condition = \"expr\"] attribute is required to encode an Option<T>");
        };
      };
      let encode = self.gen_encode_as_ty(mod_path, quote! { #ident }, &opt_ty);
      return quote! {
        if #expr {
          if let Some(ref #ident) = self.#ident {
            #encode
          }
        }
      };
    } else {
      if let Some(MetaType { ty }) = bitflags {
        return self.gen_encode_as_ty(
          mod_path,
          quote! {
            &self.#ident.bits()
          },
          ty,
        );
      }
      self.gen_encode_as_ty(
        mod_path,
        quote! {
          &self.#ident
        },
        ty,
      )
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
      flo_util::binary
    });

    let (imp, ty, wher) = generics.split_for_impl();

    match data {
      ast::Data::Struct(fields) => {
        if fields.is_newtype() {
          let repr_ty = &fields.fields.first().unwrap().ty;
          tokens.extend(quote! {
            impl #imp #mod_path::BinEncode for #ident #ty #wher {
              fn encode<T: #mod_path::BufMut>(&self, buf: &mut T) {
                <#repr_ty as #mod_path::BinEncode>::encode(&self.0, buf);
              }
            }
          })
        } else if fields.is_unit() {
          tokens.extend(quote! {
            impl #imp #mod_path::BinEncode for #ident #ty #wher {
              fn encode<T: #mod_path::BufMut>(&self, buf: &mut T) {}
            }
          })
        } else {
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
      }
      ast::Data::Enum(variants) => {
        let repr = enum_repr.as_ref().map(|v| v.ty.clone()).unwrap_or_else(|| {
          syn::parse_quote! { u32 }
        });

        // (Enum::Variant => value),*
        let items: Punctuated<TokenStream, syn::Token![,]> = variants
          .iter()
          .map(|v| {
            let vident = &v.ident;
            if vident.to_string() == "UnknownValue" {
              quote! { #ident::UnknownValue(ref v) => v.clone() }
            } else {
              if let Some(value) = v.value.as_ref() {
                return quote! { #ident::#vident => #value };
              }
              quote::quote_spanned! {
                vident.span() => compile_error!("#[bin(value = lit)] not specified");
              }
            }
          })
          .collect();

        tokens.extend(quote! {
          impl #imp #mod_path::BinEncode for #ident #ty #wher {
            fn encode<T: #mod_path::BufMut>(&self, buf: &mut T) {
              <#repr as #mod_path::BinEncode>::encode(&#repr::from(*self), buf);
            }
          }

          impl #imp From<#ident #ty> for #repr #wher {
            fn from(v: #ident) -> #repr {
              match v {
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
  condition: Option<MetaExpr>,
  #[darling(default)]
  repeat: Option<MetaExpr>,
  #[darling(default)]
  bitflags: Option<MetaType>,
}

#[derive(Debug, FromVariant)]
#[darling(attributes(bin))]
struct VariantReceiver {
  ident: syn::Ident,
  #[darling(default)]
  value: Option<syn::Lit>,
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
struct MetaType {
  ty: syn::Type,
}

impl FromMeta for MetaType {
  fn from_meta(item: &syn::Meta) -> Result<Self, Error> {
    if let syn::Meta::List(syn::MetaList { ref nested, .. }) = *item {
      if nested.len() == 1 {
        if let Some(syn::NestedMeta::Meta(syn::Meta::Path(ref path))) = nested.first() {
          return Ok(MetaType {
            ty: syn::Type::Path(syn::TypePath {
              qself: None,
              path: syn::Path::clone(path),
            }),
          });
        }
      }
    }
    Err(Error::custom("invalid syntax, `#[bin(enum_repr(Type))]` expected").with_span(item))
  }
}

#[derive(Debug)]
struct MetaExpr {
  expr: syn::Expr,
}

impl FromMeta for MetaExpr {
  fn from_string(value: &str) -> Result<Self, Error> {
    let expr =
      syn::parse_str(value).map_err(|e| Error::custom(format!("invalid syntax: {}", e)))?;
    Ok(MetaExpr { expr })
  }
}

fn is_type_likely_u8(ty: &syn::Type) -> bool {
  ty == &syn::parse_quote! { u8 }
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

fn add_self_to_idents(expr: &syn::Expr) -> syn::Expr {
  use syn::visit_mut::{visit_expr_mut, VisitMut};
  struct AddSelf;

  impl VisitMut for AddSelf {
    fn visit_expr_mut(&mut self, node: &mut syn::Expr) {
      if let syn::Expr::Path(syn::ExprPath {
        qself: None, path, ..
      }) = node.clone()
      {
        if let Some(ident) = path.get_ident() {
          *node = syn::parse_quote! {
            self.#ident
          };
          return;
        }
      }
      visit_expr_mut(self, node)
    }
  }

  let mut visitor = AddSelf;
  let mut expr = syn::Expr::clone(expr);
  visit_expr_mut(&mut visitor, &mut expr);

  expr
}
