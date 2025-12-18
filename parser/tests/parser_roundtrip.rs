use merak_ast::NodeIdGenerator;
use merak_parser::parse_program;

#[test]
fn test_var() {
    let input = r#"
    contract SimpleVault {
        state var balance: int = 0;
    }
    "#;

    let expected = r#"contract SimpleVault {
    state var balance: int = 0;
}

"#;
    let id_gen = NodeIdGenerator::new();
    let parsed = parse_program(input, &id_gen).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn test_const() {
    let input = r#"
    contract SimpleVault {
        state const maxBalance: int = 1000;
    }
    "#;

    let expected = r#"contract SimpleVault {
    state const maxBalance: int = 1000;
}

"#;
    let id_gen = NodeIdGenerator::new();
    let parsed = parse_program(input, &id_gen).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn test_constructor() {
    let input = r#"
    contract SimpleVault {
        state var balance: int = 0;
        state const maxBalance: int = 1000;

        constructor(owner: address) {
            balance = 0;
        }
    }
    "#;

    let expected = r#"contract SimpleVault {
    state var balance: int = 0;
    state const maxBalance: int = 1000;

    constructor(owner: address) {
    balance = 0;
    }
}

"#;
    let id_gen = NodeIdGenerator::new();
    let parsed = parse_program(input, &id_gen).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn simple_contract_roundtrip() {
    let source = r#"
        contract SimpleVault {
            state var balance: int = 0;
            state const maxBalance: int = 1000;

            constructor(owner: address) {
                balance = 0;
            }
            entrypoint deposit(amount: {int | amount > 0 && amount <= 100}) {
                balance = balance + amount;
            }

            external function reset() {
                balance = 0;
            }
        }
    "#;

    let id_gen = NodeIdGenerator::new();
    let first_pass = parse_program(source, &id_gen).unwrap();
    let displayed = first_pass.to_string();
    eprintln!("=== FIRST PASS OUTPUT ===\n{}\n=== END ===", displayed);
    let id_gen = NodeIdGenerator::new();

    match parse_program(&displayed, &id_gen) {
        Ok(second_pass) => {
            assert_eq!(
                first_pass.to_string(),
                second_pass.to_string(),
                "Roundtrip failed: printed code is not stable"
            );
        }
        Err(e) => {
            panic!("Second parse failed: {:?}", e);
        }
    }
}

#[test]
fn complex_arithmetic_roundtrip() {
    let source = r#"
        contract Calculator {
            state var result: int = 9;
            state var operationCount: int = 4;
            state const maxOperations: int = 100;

            constructor(initialValue: int) {
                result = initialValue;
                operationCount = 6;
            }

            external function complexOperation(a: {int | a > 2 && a < 1000}, b: {int | b != 1}) -> int {
                var temp: int = (a * b) + (a / b);
                result = result + temp;
                operationCount = operationCount + 1;

                if ((result > 5000 && a > 100) || (result < -5000 && b < -100)) {
                    return 0;
                }

                return result;
            }
        }
    "#;

    let id_gen = NodeIdGenerator::new();
    let first_pass = parse_program(source, &id_gen).unwrap();
    let id_gen = NodeIdGenerator::new();
    let second_pass = parse_program(&first_pass.to_string(), &id_gen).unwrap();

    assert_eq!(
        first_pass.to_string(),
        second_pass.to_string(),
        "Roundtrip failed: printed code is not stable"
    );
}
