extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Expr, ExprLit, Fields, Lit, Meta};

/// Attribute for specifying the CSS selector for a field
///
/// # Example
///
/// The following snippet demonstrates how to annotate fields with CSS selectors.
/// It is marked as `ignore` so docs build without pulling in external traits
/// and crates required by the derive expansion.
///
/// ```rust,ignore
/// #[derive(Scrapable)]
/// struct ArticleData {
///     #[selector = "h1.title-text"]
///     title: String,
///
///     #[selector = "#as010"]
///     abstract_text: String,
/// }
/// ```
#[proc_macro_derive(Scrapable, attributes(selector))]
pub fn derive_scrapable(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the struct
    let name = &input.ident;

    // Extract the fields and their selectors
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Scrapable can only be derived for structs with named fields"),
        },
        _ => panic!("Scrapable can only be derived for structs"),
    };

    // Generate the field extraction code
    let mut field_extractions = Vec::new();
    let mut selector_inserts = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();

        // Find the selector attribute
        let mut selector = None;
        for attr in &field.attrs {
            if attr.path().is_ident("selector") {
                if let Meta::NameValue(meta_name_value) = &attr.meta {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(lit_str),
                        ..
                    }) = &meta_name_value.value
                    {
                        selector = Some(lit_str.value());
                    }
                }
            }
        }

        let selector_value = match selector {
            Some(s) => s,
            None => panic!(
                "Field {} is missing a #[selector(\"...\")] attribute",
                field_name
            ),
        };

        // Generate code for extracting this field
        field_extractions.push(quote! {
            let #field_name = document.select(&scraper::Selector::parse(#selector_value).unwrap())
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or_default();
        });

        // Generate code for adding this selector to the HashMap
        selector_inserts.push(quote! {
            selectors.insert(#field_name_str.to_string(), #selector_value.to_string());
        });
    }

    // Generate the struct initialization
    let field_inits = fields.iter().map(|f| {
        let field_name = f.ident.as_ref().unwrap();
        quote! { #field_name }
    });

    // Generate the implementation
    let expanded = quote! {
        impl Scrapable for #name {
            fn extract_from_html(html: &str) -> Result<Self, ConnectorError> {
                // Create a scraper document
                let document = scraper::Html::parse_document(html);

                // Extract each field
                #(#field_extractions)*

                // Return the struct
                Ok(Self {
                    #(#field_inits),*
                })
            }

            fn get_selectors() -> std::collections::HashMap<String, String> {
                let mut selectors = std::collections::HashMap::new();
                #(#selector_inserts)*
                selectors
            }
        }
    };

    // Return the generated implementation
    TokenStream::from(expanded)
}
