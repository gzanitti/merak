use clap::{Parser, Subcommand};
use merak::Compiler;
use std::path::PathBuf;

/// Merak compiler CLI
#[derive(Parser)]
#[command(
    name = "merak",
    author = "Merak Project",
    version,
    about = "Command-line compiler for the Merak language",
    long_about = None,
    arg_required_else_help = false
)]
struct Cli {
    /// Source file (if passed without subcommand, defaults to 'build')
    input: Option<PathBuf>,

    /// Output directory (used only when passing a direct file)
    #[arg(short, long, default_value = "build")]
    out_dir: PathBuf,

    /// Print compiled bytecode as hex to stdout
    #[arg(short, long)]
    print_bytecode: bool,

    /// Verbose mode
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile a .mk source file
    Build {
        /// Input source file
        input: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "build")]
        out_dir: PathBuf,

        /// Print compiled bytecode as hex to stdout
        #[arg(short, long)]
        print_bytecode: bool,

        /// Verbose mode
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show compiler or project information
    Info,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Build {
            input,
            out_dir,
            print_bytecode,
            verbose,
        }) => {
            run_build(input, out_dir, print_bytecode, verbose);
        }

        Some(Commands::Info) => {
            println!("Merak compiler v{}", env!("CARGO_PKG_VERSION"));
            match std::env::current_dir() {
                Ok(dir) => println!("Workspace location: {:?}", dir),
                Err(e) => eprintln!("Failed to get current directory: {}", e),
            }
        }

        None => {
            if let Some(input) = cli.input {
                run_build(input, cli.out_dir, cli.print_bytecode, cli.verbose);
            } else {
                eprintln!(
                    "No input file provided.\nUsage: merak <file.mk> or merak build <file.mk>"
                );
            }
        }
    }
}

fn run_build(input: PathBuf, out_dir: PathBuf, print_bytecode: bool, verbose: bool) {
    if verbose {
        println!("Compiling {:?}...", input);
    }

    if !input.exists() {
        eprintln!("File {:?} not found", input);
        return;
    }

    let mut compiler = Compiler::new();
    let compiled = match compiler.compile(input.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Compilation failed for {:?}: {}", input, e);
            return;
        }
    };

    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("Could not create output directory {:?}: {}", out_dir, e);
        return;
    }

    for (contract_name, bytecode) in &compiled.contracts {
        let hex: String = bytecode.iter().map(|b| format!("{:02x}", b)).collect();

        // Default: write <contract>.bin to the output directory
        let bin_path = out_dir.join(format!("{}.bin", contract_name));
        if let Err(e) = std::fs::write(&bin_path, &hex) {
            eprintln!("Failed to write {:?}: {}", bin_path, e);
            return;
        }
        if verbose {
            println!("Written {:?} ({} bytes)", bin_path, bytecode.len());
        }

        // Optional: also print to stdout
        if print_bytecode {
            println!("{}:", contract_name);
            println!("{}", hex);
        }
    }

    if verbose {
        println!("Compilation finished. Output written to {:?}", out_dir);
    }
}
