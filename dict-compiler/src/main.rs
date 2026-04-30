mod dat_builder;
mod parser;
mod serialize;

use std::path::PathBuf;

fn main() {
    println!("dict-compiler v{}", env!("CARGO_PKG_VERSION"));
    println!();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args.contains(&"--help".to_string()) {
        eprintln!("Usage: dict-compiler [OPTIONS]");
        eprintln!("  --input <FILE>     Source word list (can repeat)");
        eprintln!("  --output <FILE>    Output .dict file (mmap binary format)");
        eprintln!("  --help             Show this help");
        return;
    }

    let mut input_files = Vec::new();
    let mut output_path = PathBuf::from("base.dict");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                i += 1;
                if i < args.len() {
                    input_files.push(PathBuf::from(&args[i]));
                }
            }
            "--output" => {
                i += 1;
                if i < args.len() {
                    output_path = PathBuf::from(&args[i]);
                }
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    if input_files.is_empty() {
        eprintln!("No input files specified. Use --input <FILE>");
        return;
    }

    println!("Input files: {:?}", input_files);
    println!("Output: {}", output_path.display());

    // --- Parse all input files ---
    let mut all_entries = Vec::new();
    for input_path in &input_files {
        match parser::parse_word_list(input_path) {
            Ok(entries) => {
                println!("  Parsed {} entries from {}", entries.len(), input_path.display());
                all_entries.extend(entries);
            }
            Err(e) => {
                eprintln!("  Error parsing {}: {}", input_path.display(), e);
            }
        }
    }

    println!("Total entries: {}", all_entries.len());

    // --- Serialize to binary DAT format ---
    let output_str = output_path.to_string_lossy().to_string();
    match serialize::serialize_to_file(&all_entries, &output_str) {
        Ok(()) => {
            let file_size = std::fs::metadata(&output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            println!(
                "\nDone. Built {} ({} bytes, {} entries)",
                output_path.display(),
                file_size,
                all_entries.len()
            );
        }
        Err(e) => {
            eprintln!("\nError: {e}");
        }
    }
}
