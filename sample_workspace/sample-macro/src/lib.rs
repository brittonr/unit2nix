use proc_macro::TokenStream;

/// Derive macro that implements `HelloMacro` trait.
/// Generates a `hello()` method returning the type's name.
#[proc_macro_derive(HelloMacro)]
pub fn hello_macro_derive(input: TokenStream) -> TokenStream {
    let input: proc_macro::TokenStream = input;
    let src = input.to_string();

    // Extract the struct name (simple parser, no syn dependency)
    let name = src
        .split_whitespace()
        .skip_while(|t| *t != "struct" && *t != "enum")
        .nth(1)
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
        .unwrap_or("Unknown");

    let expanded = format!(
        r#"
        impl {name} {{
            pub fn hello() -> &'static str {{
                concat!("Hello from ", "{name}", "!")
            }}
        }}
        "#
    );

    expanded.parse().expect("generated code should parse")
}
