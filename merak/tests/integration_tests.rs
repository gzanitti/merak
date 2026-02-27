use std::path::PathBuf;
use std::fs;

use merak::Compiler;

#[test]
fn test_basic_example_integration() {
    let mut compiler = Compiler::new();
    compiler
        .compile(PathBuf::from("../examples/basic_vault.merak"))
        .unwrap();
}

#[test]
fn test_all_examples_compile() {
    let examples_dir = PathBuf::from("../examples");

    // Get all .merak files in the examples directory
    let entries = fs::read_dir(&examples_dir)
        .expect("Failed to read examples directory");

    let mut merak_files: Vec<PathBuf> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "merak" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    // Sort for consistent test order
    merak_files.sort();

    assert!(!merak_files.is_empty(), "No .merak files found in examples directory");

    // Compile each example
    for example_file in merak_files {
        println!("\n=== Testing example: {:?} ===", example_file.file_name().unwrap());

        let mut compiler = Compiler::new();
        let result = compiler.compile(example_file.clone());

        // Check if this is an expected-to-fail test
        let file_name = example_file.file_name().unwrap().to_str().unwrap();
        if file_name.starts_with("test_invalid") || file_name.starts_with("test_conflicting") {
            // These files are expected to fail validation
            assert!(
                result.is_err(),
                "Expected {:?} to fail but it succeeded",
                file_name
            );
            println!("✓ Example correctly failed: {:?}", file_name);
        } else {
            // All other examples should compile successfully
            assert!(
                result.is_ok(),
                "Failed to compile {:?}: {:?}",
                file_name,
                result.err()
            );
            println!("✓ Example compiled successfully: {:?}", file_name);
        }
    }
}
