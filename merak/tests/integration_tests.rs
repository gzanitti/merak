use std::path::PathBuf;

use merak::Compiler;

#[test]
fn test_basic_example_integration() {
    let mut compiler = Compiler::new();
    compiler
        .compile(PathBuf::from("../examples/basic_vault.merak"))
        .unwrap();
}
