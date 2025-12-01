use merak_parser::parse_program;

#[test]
fn test_empty_states() {
    let input = r#"
    contract SimpleVault[] {
        state var balance: int = 0;
    }"#;

    let expected = r#"contract SimpleVault[] {
    state var balance: int = 0;
}

"#;
    let parsed = parse_program(input).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn test_var() {
    let input = r#"
    contract SimpleVault[Open, Closed] {
        state var balance: int = 0;
    }
    "#;

    let expected = r#"contract SimpleVault[Open, Closed] {
    state var balance: int = 0;
}

"#;
    let parsed = parse_program(input).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn test_const() {
    let input = r#"
    contract SimpleVault[Open, Closed] {
        state const maxBalance: int = 1000;
    }
    "#;

    let expected = r#"contract SimpleVault[Open, Closed] {
    state const maxBalance: int = 1000;
}

"#;
    let parsed = parse_program(input).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn test_constructor() {
    let input = r#"
    contract SimpleVault[Open, Closed] {
        state var balance: int = 0;
        state const maxBalance: int = 1000;

        constructor(owner: address) {
            balance = 0;
        }
    }
    "#;

    let expected = r#"contract SimpleVault[Open, Closed] {
    state var balance: int = 0;
    state const maxBalance: int = 1000;

    constructor(owner: address) {
    balance = 0;
    }
}

"#;
    let parsed = parse_program(input).unwrap();
    assert_eq!(parsed.to_string(), expected);
}

#[test]
fn simple_contract_roundtrip() {
    let source = r#"
        contract SimpleVault[Open, Closed] {
            state var balance: int = 0;
            state const maxBalance: int = 1000;

            constructor(owner: address) {
                balance = 0;
            }
        }

        SimpleVault@Open(any) {
            entrypoint deposit(amount: {int | amount > 0 && amount <= 100}) {
                balance = balance + amount;
                if (balance >= maxBalance) {
                    become Closed;
                }
            }
        }

        SimpleVault@Closed(any) {
            entrypoint reset() {
                balance = 0;
                become Open;
            }
        }
    "#;

    let first_pass = parse_program(source).unwrap();
    let displayed = first_pass.to_string();
    eprintln!("=== FIRST PASS OUTPUT ===\n{}\n=== END ===", displayed);
    match parse_program(&displayed) {
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
fn token_vault_roundtrip() {
    let source = r#"
        contract TokenVault[Available, Withdrawn] {
            state var balance: int = 100;
            state const limit: int = 1000;

            constructor(owner: address) {
                balance = 0;
            }
        }

        TokenVault@Available(any) {
            entrypoint deposit(amount: int) {
                balance = balance + amount;
            }

            entrypoint withdraw(amount: {int | amount > 0 && amount <= balance}) {
                balance = balance - amount;
                if (balance == 0) {
                    become Withdrawn;
                }
            }
        }

        TokenVault@Withdrawn(any) {
            entrypoint reopen() {
                become Available;
            }
        }
    "#;

    let first_pass = parse_program(source).unwrap();
    let second_pass = parse_program(&first_pass.to_string()).unwrap();

    assert_eq!(
        first_pass.to_string(),
        second_pass.to_string(),
        "Roundtrip failed: printed code is not stable"
    );
}

#[test]
fn complex_arithmetic_roundtrip() {
    let source = r#"
        contract Calculator[Active, Inactive] {
            state var result: int = 9;
            state var operationCount: int = 4;
            state const maxOperations: int = 100;

            constructor(initialValue: int) {
                result = initialValue;
                operationCount = 6;
            }
        }

        Calculator@Active(any) {
            entrypoint add(a: int, b: int) -> int {
                result = result + a + b;
                operationCount = operationCount + 1;

                if (operationCount >= maxOperations) {
                    become Inactive;
                }

                return result;
            }

            entrypoint multiply(a: int, b: int) -> int {
                result = result * a * b;
                operationCount = operationCount + 1;

                if (operationCount >= maxOperations || result > 1000000) {
                    become Inactive;
                }

                return result;
            }

            entrypoint complexOperation(a: {int | a > 2 && a < 1000}, b: {int | b != 1}) -> int {
                var temp: int = (a * b) + (a / b);
                result = result + temp;
                operationCount = operationCount + 1;

                if ((result > 5000 && a > 100) || (result < -5000 && b < -100)) {
                    become Inactive;
                }

                return result;
            }
        }

        Calculator@Inactive(any) {
            entrypoint reset() {
                result = 0;
                operationCount = 0;
                become Active;
            }
        }
    "#;

    let first_pass = parse_program(source).unwrap();
    let second_pass = parse_program(&first_pass.to_string()).unwrap();

    assert_eq!(
        first_pass.to_string(),
        second_pass.to_string(),
        "Roundtrip failed: printed code is not stable"
    );
}
