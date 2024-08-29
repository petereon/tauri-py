use core::panic;
use quote::{format_ident, quote, ToTokens};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use syn::{
    parse_file, AngleBracketedGenericArguments, GenericArgument, Ident, Item, ItemMod, PatIdent,
    PathArguments, PathSegment, ReturnType, Type,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PYTHONPATH", "./");
    std::env::set_var("PYTHONDONTWRITEBYTECODE", "1");

    pyo3_bindgen::Codegen::default()
        .module_name("python.src")
        .unwrap()
        .build("src/gen/py_bindings.rs")
        .unwrap();

    generate_commands_from_py_bindings(
        "src/gen/py_bindings.rs",
        "src/gen/py_commands.rs",
        vec!["python", "src"],
    )
    .expect("Failed to generate Tauri commands");

    protobuf_codegen::Codegen::new()
        .out_dir("src/gen/state")
        .inputs(&["state.proto"])
        .includes(&["."])
        .run()
        .expect("Failed to generate protobuf code");

    gen_python_from_proto("state.proto", "python/src/gen", ".");

    format("src/gen/py_bindings.rs");
    format("src/gen/py_commands.rs");

    tauri_build::build();

    Ok(())
}

fn gen_python_from_proto(file: &str, out_dir: &str, proto_path: &str) {
    let output = Command::new("protoc")
        .arg(format!("--proto_path={}", proto_path))
        .arg(format!("--python_out={}", out_dir))
        .arg(format!("--mypy_out={}", out_dir))
        .arg(file)
        .output()
        .expect("Failed to execute protoc");

    if !output.status.success() {
        panic!(
            "Failed to generate Python code from proto file: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
    modules: Vec<&str>,
) -> Result<(), Box<dyn Error>> {
    // Read the input Rust file into a string
    let mut input_file = File::open(input_path)?;
    let mut input_code = String::new();
    input_file.read_to_string(&mut input_code)?;
    let mut output_code = String::new();

    // Parse the input Rust code into a syntax tree
    let syntax_tree = parse_file(&input_code)?;

    let module_idents: Vec<Ident> = modules.iter().map(|m| format_ident!("{}", m)).collect();

    // Generate the output code

    output_code.push_str(
        &quote! {
         use crate::gen::py_bindings::#(#module_idents)::* as bindings;
        }
        .to_string(),
    );

    let mut modules_iter = modules.iter();
    let first_mod_name = modules_iter.next().unwrap();

    let first_mod = get_first_mod(syntax_tree, first_mod_name);

    let mut module = first_mod;

    for module_name in modules_iter {
        module = get_tail_mod(&module, module_name);
    }

    // Process items in the module
    if let Some((_, items)) = module.clone().content {
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
                            bindings::#func_name(py, #(#args_list),*).map_err(|e| e.to_string())
                        })
                    }
                };

                // Append the transformed function to the output code
                output_code.push_str(&transformed_fn.to_string());
                output_code.push_str("\n\n");
            }
        }
    }

    // Write the transformed code to the output file
    let mut output_file = File::create(output_path)?;
    output_file.write_all(output_code.as_bytes())?;

    Ok(())
}

fn get_tail_mod(module: &ItemMod, module_name: &&str) -> ItemMod {
    module
        .content
        .as_ref()
        .unwrap()
        .1
        .iter()
        .find_map(|item| match item {
            Item::Mod(item_mod) => {
                if item_mod.ident == module_name {
                    return Some(item_mod);
                }
                None
            }
            _ => None,
        })
        .unwrap()
        .clone()
}

fn get_first_mod(syntax_tree: syn::File, first_mod_name: &str) -> syn::ItemMod {
    syntax_tree
        .items
        .into_iter()
        .find_map(|item| match item {
            syn::Item::Mod(item_mod) => {
                if item_mod.ident == first_mod_name {
                    Some(item_mod)
                } else {
                    None
                }
            }
            _ => None,
        })
        .unwrap()
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
