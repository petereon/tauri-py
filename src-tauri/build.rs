use core::panic;
use pyo3_bindgen::Codegen;
use quote::{quote, ToTokens};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use syn::{
    parse_file, AngleBracketedGenericArguments, GenericArgument, Item, PatIdent, PathArguments,
    PathSegment, ReturnType, Type,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PYTHONPATH", "./");
    std::env::set_var("PYTHONDONTWRITEBYTECODE", "1");

    Codegen::default()
        .module_name("src_py")
        .unwrap()
        .build("src/gen/py_bindings.rs")
        .unwrap();

    format("src/gen/py_bindings.rs");

    generate_commands_from_py_bindings("src/gen/py_bindings.rs", "src/gen/py_commands.rs")
        .expect("Failed to generate Tauri commands");

    format("src/gen/py_commands.rs");

    tauri_build::build();

    Ok(())
}

fn format(path: &str) {
    let output = Command::new("rustfmt")
        .arg(path)
        .output()
        .expect("Failed to run rustfmt");

    if !output.status.success() {
        panic!("Failed to run rustfmt");
    }
}

/// Transforms Rust code from the input file according to the specified pattern
/// and writes the transformed code to the output file.
fn generate_commands_from_py_bindings<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
) -> Result<(), Box<dyn Error>> {
    // Read the input Rust file into a string
    let mut input_file = File::open(input_path)?;
    let mut input_code = String::new();
    input_file.read_to_string(&mut input_code)?;

    // Parse the input Rust code into a syntax tree
    let syntax_tree = parse_file(&input_code)?;

    // Generate the output code
    let mut output_code = String::new();

    output_code.push_str(
        &quote! {
         use crate::gen::py_bindings::src_py;
        }
        .to_string(),
    );

    // Process items in the syntax tree
    for item in syntax_tree.items {
        if let Item::Mod(item_mod) = item {
            // Add use statement for the module

            // Process items in the module
            if let Some((_, items)) = item_mod.content {
                for item in items {
                    if let Item::Fn(func) = item {
                        // Skip functions that don't match the expected pattern
                        if func.sig.inputs.len() < 2 {
                            continue;
                        }

                        // Extract function name, arguments, and return type
                        let func_name = &func.sig.ident;
                        let args = &func.sig.inputs;
                        let mut args_iter = args.iter();
                        let _ = args_iter.next(); // Skip the first argument
                        let remaining_args: Vec<_> = args_iter.map(replace_prefix).collect();

                        let ret_type = match &func.sig.output {
                            ReturnType::Type(_, ty) => extract_path_segment(*ty.clone()),
                            ReturnType::Default => panic!("Function must have a return type"),
                        };

                        // Convert function arguments to appropriate quote format
                        let args_list = remaining_args.iter().map(|arg| {
                            let arg_name = match arg {
                                syn::FnArg::Typed(pat_type) => &pat_type.pat,
                                _ => panic!("Unexpected argument type"),
                            };
                            quote! { #arg_name }
                        });

                        // Build the transformed function
                        let transformed_fn = quote! {
                            #[tauri::command]
                            pub fn #func_name(#(#remaining_args),*) -> Result<#ret_type, String> {
                                pyo3::Python::with_gil(|py| {
                                    src_py::#func_name(py, #(#args_list),*).map_err(|e| e.to_string())
                                })
                            }
                        };

                        // Append the transformed function to the output code
                        output_code.push_str(&transformed_fn.to_string());
                        output_code.push_str("\n\n");
                    }
                }
            }
        }
    }

    // Write the transformed code to the output file
    let mut output_file = File::create(output_path)?;
    output_file.write_all(output_code.as_bytes())?;

    Ok(())
}

fn replace_prefix(arg: &syn::FnArg) -> syn::FnArg {
    match arg {
        syn::FnArg::Typed(pat_type) => {
            let arg_name = &pat_type.pat.to_token_stream().to_string().replace("p_", "");
            let ty = &pat_type.ty;
            syn::FnArg::Typed(syn::PatType {
                attrs: Vec::new(), // Attributes, if any
                pat: Box::new(syn::Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    by_ref: None,
                    mutability: None,
                    ident: syn::Ident::new(arg_name, proc_macro2::Span::call_site()),
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(*ty.clone()),
            })
        }
        _ => panic!("Unexpected argument type"),
    }
}

fn extract_path_segment(ty: Type) -> Option<PathSegment> {
    if let Type::Path(type_path) = ty {
        for segment in type_path.path.segments {
            if let PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) =
                &segment.arguments
            {
                if let Some(GenericArgument::Type(Type::Path(inner_path))) = args.first() {
                    return inner_path.path.segments.last().cloned();
                }
            }
        }
    }
    None
}
