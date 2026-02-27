/// ABI encoding utilities for Ethereum function calls
///
/// TODO: Implement ABI encoding/decoding:
/// - Function selector computation (keccak256(signature)[0..4])
/// - Argument encoding (following Solidity ABI spec)
/// - Return value decoding

use merak_ast::types::BaseType;
use merak_symbols::{SymbolId, SymbolKind, SymbolTable};
use tiny_keccak::{Hasher, Keccak};

use crate::CodegenError;

/// Compute function selector from signature
/// Example: "transfer(address,uint256)" -> 0xa9059cbb
pub fn compute_selector(signature: &str) -> [u8; 4] {
    let mut keccak = Keccak::v256();
    let mut output = [0u8; 32];
    keccak.update(signature.as_bytes());
    keccak.finalize(&mut output);

    [output[0], output[1], output[2], output[3]]
}

/// Convert Merak BaseType to Solidity ABI type string
/// Used for generating function signatures for selector computation
fn base_type_to_abi_string(base_type: &BaseType) -> Result<String, CodegenError> {
    match base_type {
        BaseType::Int => Ok("uint256".to_string()),
        BaseType::Address => Ok("address".to_string()),
        BaseType::Bool => Ok("bool".to_string()),
        BaseType::String => Ok("string".to_string()),
        BaseType::Contract(_) => Ok("address".to_string()), // Contracts are addresses in EVM
        BaseType::Tuple { elems } => {
            let elem_types: Result<Vec<String>, CodegenError> = elems
                .iter()
                .map(|ty| base_type_to_abi_string(&ty.base))
                .collect();
            Ok(format!("({})", elem_types?.join(",")))
        }
        BaseType::Function { .. } => Err(CodegenError::Other(
            "Function types are not supported in ABI signatures".to_string(),
        )),
    }
}

/// Build ABI signature for a function from symbol table
/// Format: "functionName(type1,type2,...)"
///
/// Example: For a function `transfer(to: address, amount: int)`, returns "transfer(address,uint256)"
pub fn build_abi_signature(
    function_name: &str,
    symbol_table: &SymbolTable,
    function_id: &SymbolId,
) -> Result<String, CodegenError> {
    let symbol_info = symbol_table.get_symbol(function_id);

    // Extract parameters from SymbolKind
    let parameters = match &symbol_info.kind {
        SymbolKind::Function { parameters, .. } | SymbolKind::Entrypoint { parameters, .. } => {
            parameters
        }
        _ => {
            return Err(CodegenError::Other(format!(
                "Symbol {:?} is not a function or entrypoint",
                function_id
            )))
        }
    };

    // Map parameters to ABI types (extracting base type from liquid types)
    let param_types: Result<Vec<String>, CodegenError> = parameters
        .iter()
        .map(|param| base_type_to_abi_string(&param.ty.base))
        .collect();

    let param_types = param_types?;
    let signature = format!("{}({})", function_name, param_types.join(","));
    Ok(signature)
}

// TODO: Implement ABI encoding functions
// pub fn encode_args(args: &[Value]) -> Vec<u8> { ... }
// pub fn decode_return(data: &[u8], ty: &Type) -> Value { ... }

#[cfg(test)]
mod tests {
    use super::*;
    use merak_ast::{contract::Param, function::Visibility, node_id::NodeId, predicate::Predicate, types::Type, meta::SourceRef};
    use merak_symbols::{QualifiedName, SymbolKind};

    #[test]
    fn test_selector_computation() {
        // Known selector for "transfer(address,uint256)"
        let selector = compute_selector("transfer(address,uint256)");
        assert_eq!(selector, [0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_balance_of_selector() {
        // balanceOf(address) -> 0x70a08231
        let selector = compute_selector("balanceOf(address)");
        assert_eq!(selector, [0x70, 0xa0, 0x82, 0x31]);
    }

    #[test]
    fn test_base_type_to_abi_string_primitives() {
        // Test basic types
        assert_eq!(base_type_to_abi_string(&BaseType::Int).unwrap(), "uint256");
        assert_eq!(base_type_to_abi_string(&BaseType::Address).unwrap(), "address");
        assert_eq!(base_type_to_abi_string(&BaseType::Bool).unwrap(), "bool");
        assert_eq!(base_type_to_abi_string(&BaseType::String).unwrap(), "string");
    }

    #[test]
    fn test_base_type_to_abi_string_contract() {
        // Contracts are represented as addresses in ABI
        let contract_type = BaseType::Contract("Token".to_string());
        assert_eq!(base_type_to_abi_string(&contract_type).unwrap(), "address");
    }

    #[test]
    fn test_base_type_to_abi_string_tuple() {
        // Test tuple type: (uint256, address)
        let tuple_type = BaseType::Tuple {
            elems: vec![
                Type {
                    base: BaseType::Int,
                    binder: "x".to_string(),
                    constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                    explicit_annotation: false,
                    source_ref: SourceRef::unknown(),
                },
                Type {
                    base: BaseType::Address,
                    binder: "y".to_string(),
                    constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                    explicit_annotation: false,
                    source_ref: SourceRef::unknown(),
                },
            ],
        };
        assert_eq!(base_type_to_abi_string(&tuple_type).unwrap(), "(uint256,address)");
    }

    #[test]
    fn test_base_type_to_abi_string_function_error() {
        // Function types should return error
        let func_type = BaseType::Function {
            name: "foo".to_string(),
            parameters: vec![],
            return_type: Box::new(Type {
                base: BaseType::Bool,
                binder: "r".to_string(),
                constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                explicit_annotation: false,
                source_ref: SourceRef::unknown(),
            }),
        };
        assert!(base_type_to_abi_string(&func_type).is_err());
    }

    #[test]
    fn test_build_abi_signature_simple() {
        // Create a simple function: transfer(address, uint256)
        let mut symbol_table = SymbolTable::new();

        let param1 = Param {
            name: "to".to_string(),
            ty: Type {
                base: BaseType::Address,
                binder: "to".to_string(),
                constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                explicit_annotation: false,
                source_ref: SourceRef::unknown(),
            },
            id: NodeId::from(1),
            source_ref: SourceRef::unknown(),
        };

        let param2 = Param {
            name: "amount".to_string(),
            ty: Type {
                base: BaseType::Int,
                binder: "amount".to_string(),
                constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                explicit_annotation: false,
                source_ref: SourceRef::unknown(),
            },
            id: NodeId::from(2),
            source_ref: SourceRef::unknown(),
        };

        let func_id = symbol_table
            .add_symbol(
                NodeId::from(100),
                QualifiedName::from_string("transfer".to_string()),
                SymbolKind::Function {
                    visibility: Visibility::External,
                    reentrancy: merak_ast::function::Modifier::Checked,
                    parameters: vec![param1, param2],
                    ensures: vec![],
                    requires: vec![],
                    return_type: Type {
                        base: BaseType::Bool,
                        binder: "r".to_string(),
                        constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                        explicit_annotation: false,
                        source_ref: SourceRef::unknown(),
                    },
                },
                None,
            )
            .unwrap();

        let signature = build_abi_signature("transfer", &symbol_table, &func_id).unwrap();
        assert_eq!(signature, "transfer(address,uint256)");

        // Verify the selector matches expected value
        let selector = compute_selector(&signature);
        assert_eq!(selector, [0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_build_abi_signature_no_params() {
        // Create a function with no parameters: getBalance()
        let mut symbol_table = SymbolTable::new();

        let func_id = symbol_table
            .add_symbol(
                NodeId::from(100),
                QualifiedName::from_string("getBalance".to_string()),
                SymbolKind::Entrypoint {
                    reentrancy: merak_ast::function::Modifier::Checked,
                    parameters: vec![],
                    ensures: vec![],
                    requires: vec![],
                    return_type: Type {
                        base: BaseType::Int,
                        binder: "r".to_string(),
                        constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                        explicit_annotation: false,
                        source_ref: SourceRef::unknown(),
                    },
                },
                None,
            )
            .unwrap();

        let signature = build_abi_signature("getBalance", &symbol_table, &func_id).unwrap();
        assert_eq!(signature, "getBalance()");
    }

    #[test]
    fn test_build_abi_signature_liquid_types() {
        // Test that liquid types are stripped to base types
        // Function: deposit(amount: {x: int | x > 0})
        let mut symbol_table = SymbolTable::new();

        let param = Param {
            name: "amount".to_string(),
            ty: Type {
                base: BaseType::Int,
                binder: "x".to_string(),
                // Refinement constraint: x > 0 (liquid type)
                constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()), // Simplified for test
                explicit_annotation: true,
                source_ref: SourceRef::unknown(),
            },
            id: NodeId::from(1),
            source_ref: SourceRef::unknown(),
        };

        let func_id = symbol_table
            .add_symbol(
                NodeId::from(100),
                QualifiedName::from_string("deposit".to_string()),
                SymbolKind::Function {
                    visibility: Visibility::External,
                    reentrancy: merak_ast::function::Modifier::Checked,
                    parameters: vec![param],
                    ensures: vec![],
                    requires: vec![],
                    return_type: Type {
                        base: BaseType::Bool,
                        binder: "r".to_string(),
                        constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                        explicit_annotation: false,
                        source_ref: SourceRef::unknown(),
                    },
                },
                None,
            )
            .unwrap();

        // The signature should use the base type (int -> uint256), ignoring refinements
        let signature = build_abi_signature("deposit", &symbol_table, &func_id).unwrap();
        assert_eq!(signature, "deposit(uint256)");
    }
}
