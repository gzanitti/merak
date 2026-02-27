use std::{collections::HashMap, path::PathBuf};

use indexmap::IndexMap;
use merak_analyzer::{analyze, analyze_ssa};
use merak_ast::{NodeIdGenerator, contract::Program};
use merak_codegen::Codegen;
use merak_errors::MerakError;
use merak_ir::transformers::ssa::SsaBuilder;
use merak_parser::parse_file;

// Re-export for external use
pub use merak_ast;
pub use merak_codegen::CompiledProgram;
pub use merak_errors;

#[derive(Debug)]
pub struct Compiler;

impl Compiler {
    pub fn new() -> Self {
        Self
    }

    pub fn compile(&mut self, entry: PathBuf) -> Result<CompiledProgram, MerakError> {
        println!("Compiling {:?}", entry);
        let loaded = load_program(&entry)?;

        // Phase 1-3: Symbol collection, type checking
        let symbol_table = analyze(&loaded)?;

        // Phase 4-6: CFG construction, dominance analysis, SSA transform
        let mut ssa_program = SsaBuilder::new(symbol_table.clone()).build(&loaded)?;
        println!("Finished generating SSA IR...");

        // Phase 7: Storage analysis (CEI pattern, immutability, fold/unfold insertion)
        for (_, file) in &mut ssa_program.files {
            analyze_ssa(&mut file.contract, &symbol_table)
                .map_err(|e| MerakError::InternalError(format!("Storage analysis: {e}")))?;
        }
        println!("Finished storage analysis...");

        // Phase 8: Refinement checking (VC generation + SMT verification)
        // TODO: not yet fully implemented
        // merak_analyzer::refinements::check(&ssa_program, &symbol_table)?;

        // Phase 9: Codegen — lower SSA to EVM bytecode
        let compiled = Codegen::new()
            .compile_program(&mut ssa_program, &symbol_table)
            .map_err(|e| MerakError::InternalError(format!("Codegen: {:?}", e)))?;
        println!("Finished generating EVM bytecode...");

        Ok(compiled)
    }
}

fn load_program(entry_path: &PathBuf) -> Result<Program, MerakError> {
    let mut loaded = Program {
        files: IndexMap::new(),
    };

    let mut visited = HashMap::new();
    let id_gen = NodeIdGenerator::new();
    load_recursive(entry_path, &mut loaded, &mut visited, None, &id_gen)?;

    Ok(loaded)
}

pub fn load_recursive(
    file_path: &PathBuf,
    loaded: &mut Program,
    visited: &mut HashMap<PathBuf, bool>,
    alias: Option<String>,
    id_gen: &NodeIdGenerator,
) -> Result<(), MerakError> {
    if visited.contains_key(file_path) {
        return Ok(());
    }
    visited.insert(file_path.clone(), true);

    let file = parse_file(&file_path, &id_gen)?;

    // If available, use the alias provided by the import
    let contract_key = alias.unwrap_or_else(|| file.contract.name.clone());

    println!("Import path: {}", file_path.display());
    

    for imp in &file.imports {
        let import_path = resolve_import_path(file_path, &imp.file_path)?;
        load_recursive(&import_path, loaded, visited, imp.alias.clone(), &id_gen)?;
    }

    loaded.files.insert(contract_key, file);

    Ok(())
}

/// Resolve import path relative to current_file.
/// Rules:
/// - if import_path starts with "./" or "../" or is relative, resolve against current_file.parent()
/// - if import_path is absolute, use as-is
/// - always append extension ".merak" if no extension present
fn resolve_import_path(
    current_file: &PathBuf,
    import_path: &PathBuf,
) -> Result<PathBuf, MerakError> {
    // parent dir of current file
    let base_dir = current_file
        .parent()
        .ok_or_else(|| MerakError::InvalidPath(current_file.to_path_buf()))?;

    // If import_path is relative (starts with "." or no root), join with base_dir
    // We treat "simple_vault" as relative to current_file as per our convention.
    let candidate = if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        base_dir.join(import_path)
    };

    // Add extension if none
    let candidate = if candidate.extension().is_none() {
        candidate.with_extension("merak")
    } else {
        candidate
    };

    // Canonicalize for uniform keys (resolves symlinks; may error if file doesn't exist)
    let canon =
        std::fs::canonicalize(&candidate).map_err(|_| MerakError::NotFound(candidate.clone()))?;

    Ok(canon)
}
