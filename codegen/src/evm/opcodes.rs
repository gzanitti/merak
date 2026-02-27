/// EVM opcodes enumeration with encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Opcode {
    // 0s: Stop and Arithmetic Operations
    STOP,       // 0x00
    ADD,        // 0x01
    MUL,        // 0x02
    SUB,        // 0x03
    DIV,        // 0x04
    SDIV,       // 0x05
    MOD,        // 0x06
    SMOD,       // 0x07
    ADDMOD,     // 0x08
    MULMOD,     // 0x09
    EXP,        // 0x0a
    SIGNEXTEND, // 0x0b

    // 10s: Comparison & Bitwise Logic Operations
    LT,     // 0x10
    GT,     // 0x11
    SLT,    // 0x12
    SGT,    // 0x13
    EQ,     // 0x14
    ISZERO, // 0x15
    AND,    // 0x16
    OR,     // 0x17
    XOR,    // 0x18
    NOT,    // 0x19
    BYTE,   // 0x1a
    SHL,    // 0x1b
    SHR,    // 0x1c
    SAR,    // 0x1d

    // 20s: SHA3
    SHA3, // 0x20 (KECCAK256)

    // 30s: Environmental Information
    ADDRESS,        // 0x30
    BALANCE,        // 0x31
    ORIGIN,         // 0x32
    CALLER,         // 0x33
    CALLVALUE,      // 0x34
    CALLDATALOAD,   // 0x35
    CALLDATASIZE,   // 0x36
    CALLDATACOPY,   // 0x37
    CODESIZE,       // 0x38
    CODECOPY,       // 0x39
    GASPRICE,       // 0x3a
    EXTCODESIZE,    // 0x3b
    EXTCODECOPY,    // 0x3c
    RETURNDATASIZE, // 0x3d
    RETURNDATACOPY, // 0x3e
    EXTCODEHASH,    // 0x3f

    // 40s: Block Information
    BLOCKHASH,   // 0x40
    COINBASE,    // 0x41
    TIMESTAMP,   // 0x42
    NUMBER,      // 0x43
    DIFFICULTY,  // 0x44 (PREVRANDAO post-merge)
    GASLIMIT,    // 0x45
    CHAINID,     // 0x46
    SELFBALANCE, // 0x47
    BASEFEE,     // 0x48

    // 50s: Stack, Memory, Storage and Flow Operations
    POP,      // 0x50
    MLOAD,    // 0x51
    MSTORE,   // 0x52
    MSTORE8,  // 0x53
    SLOAD,    // 0x54
    SSTORE,   // 0x55
    JUMP,     // 0x56
    JUMPI,    // 0x57
    PC,       // 0x58
    MSIZE,    // 0x59
    GAS,      // 0x5a
    JUMPDEST, // 0x5b

    // 60s & 70s: Push Operations
    PUSH1,  // 0x60
    PUSH2,  // 0x61
    PUSH3,  // 0x62
    PUSH4,  // 0x63
    PUSH5,  // 0x64
    PUSH6,  // 0x65
    PUSH7,  // 0x66
    PUSH8,  // 0x67
    PUSH9,  // 0x68
    PUSH10, // 0x69
    PUSH11, // 0x6a
    PUSH12, // 0x6b
    PUSH13, // 0x6c
    PUSH14, // 0x6d
    PUSH15, // 0x6e
    PUSH16, // 0x6f
    PUSH17, // 0x70
    PUSH18, // 0x71
    PUSH19, // 0x72
    PUSH20, // 0x73
    PUSH21, // 0x74
    PUSH22, // 0x75
    PUSH23, // 0x76
    PUSH24, // 0x77
    PUSH25, // 0x78
    PUSH26, // 0x79
    PUSH27, // 0x7a
    PUSH28, // 0x7b
    PUSH29, // 0x7c
    PUSH30, // 0x7d
    PUSH31, // 0x7e
    PUSH32, // 0x7f

    // 80s: Duplication Operations
    DUP1,  // 0x80
    DUP2,  // 0x81
    DUP3,  // 0x82
    DUP4,  // 0x83
    DUP5,  // 0x84
    DUP6,  // 0x85
    DUP7,  // 0x86
    DUP8,  // 0x87
    DUP9,  // 0x88
    DUP10, // 0x89
    DUP11, // 0x8a
    DUP12, // 0x8b
    DUP13, // 0x8c
    DUP14, // 0x8d
    DUP15, // 0x8e
    DUP16, // 0x8f

    // 90s: Exchange Operations
    SWAP1,  // 0x90
    SWAP2,  // 0x91
    SWAP3,  // 0x92
    SWAP4,  // 0x93
    SWAP5,  // 0x94
    SWAP6,  // 0x95
    SWAP7,  // 0x96
    SWAP8,  // 0x97
    SWAP9,  // 0x98
    SWAP10, // 0x99
    SWAP11, // 0x9a
    SWAP12, // 0x9b
    SWAP13, // 0x9c
    SWAP14, // 0x9d
    SWAP15, // 0x9e
    SWAP16, // 0x9f

    // a0s: Logging Operations
    LOG0, // 0xa0
    LOG1, // 0xa1
    LOG2, // 0xa2
    LOG3, // 0xa3
    LOG4, // 0xa4

    // f0s: System Operations
    CREATE,       // 0xf0
    CALL,         // 0xf1
    CALLCODE,     // 0xf2
    RETURN,       // 0xf3
    DELEGATECALL, // 0xf4
    CREATE2,      // 0xf5
    STATICCALL,   // 0xfa
    REVERT,       // 0xfd
    INVALID,      // 0xfe
    SELFDESTRUCT, // 0xff
}

impl Opcode {
    /// Encode opcode to its byte value
    pub fn encode(&self) -> u8 {
        match self {
            // 0s: Stop and Arithmetic
            Opcode::STOP => 0x00,
            Opcode::ADD => 0x01,
            Opcode::MUL => 0x02,
            Opcode::SUB => 0x03,
            Opcode::DIV => 0x04,
            Opcode::SDIV => 0x05,
            Opcode::MOD => 0x06,
            Opcode::SMOD => 0x07,
            Opcode::ADDMOD => 0x08,
            Opcode::MULMOD => 0x09,
            Opcode::EXP => 0x0a,
            Opcode::SIGNEXTEND => 0x0b,

            // 10s: Comparison & Bitwise Logic
            Opcode::LT => 0x10,
            Opcode::GT => 0x11,
            Opcode::SLT => 0x12,
            Opcode::SGT => 0x13,
            Opcode::EQ => 0x14,
            Opcode::ISZERO => 0x15,
            Opcode::AND => 0x16,
            Opcode::OR => 0x17,
            Opcode::XOR => 0x18,
            Opcode::NOT => 0x19,
            Opcode::BYTE => 0x1a,
            Opcode::SHL => 0x1b,
            Opcode::SHR => 0x1c,
            Opcode::SAR => 0x1d,

            // 20s: SHA3
            Opcode::SHA3 => 0x20,

            // 30s: Environmental Information
            Opcode::ADDRESS => 0x30,
            Opcode::BALANCE => 0x31,
            Opcode::ORIGIN => 0x32,
            Opcode::CALLER => 0x33,
            Opcode::CALLVALUE => 0x34,
            Opcode::CALLDATALOAD => 0x35,
            Opcode::CALLDATASIZE => 0x36,
            Opcode::CALLDATACOPY => 0x37,
            Opcode::CODESIZE => 0x38,
            Opcode::CODECOPY => 0x39,
            Opcode::GASPRICE => 0x3a,
            Opcode::EXTCODESIZE => 0x3b,
            Opcode::EXTCODECOPY => 0x3c,
            Opcode::RETURNDATASIZE => 0x3d,
            Opcode::RETURNDATACOPY => 0x3e,
            Opcode::EXTCODEHASH => 0x3f,

            // 40s: Block Information
            Opcode::BLOCKHASH => 0x40,
            Opcode::COINBASE => 0x41,
            Opcode::TIMESTAMP => 0x42,
            Opcode::NUMBER => 0x43,
            Opcode::DIFFICULTY => 0x44,
            Opcode::GASLIMIT => 0x45,
            Opcode::CHAINID => 0x46,
            Opcode::SELFBALANCE => 0x47,
            Opcode::BASEFEE => 0x48,

            // 50s: Stack, Memory, Storage and Flow
            Opcode::POP => 0x50,
            Opcode::MLOAD => 0x51,
            Opcode::MSTORE => 0x52,
            Opcode::MSTORE8 => 0x53,
            Opcode::SLOAD => 0x54,
            Opcode::SSTORE => 0x55,
            Opcode::JUMP => 0x56,
            Opcode::JUMPI => 0x57,
            Opcode::PC => 0x58,
            Opcode::MSIZE => 0x59,
            Opcode::GAS => 0x5a,
            Opcode::JUMPDEST => 0x5b,

            // 60s & 70s: Push Operations
            Opcode::PUSH1 => 0x60,
            Opcode::PUSH2 => 0x61,
            Opcode::PUSH3 => 0x62,
            Opcode::PUSH4 => 0x63,
            Opcode::PUSH5 => 0x64,
            Opcode::PUSH6 => 0x65,
            Opcode::PUSH7 => 0x66,
            Opcode::PUSH8 => 0x67,
            Opcode::PUSH9 => 0x68,
            Opcode::PUSH10 => 0x69,
            Opcode::PUSH11 => 0x6a,
            Opcode::PUSH12 => 0x6b,
            Opcode::PUSH13 => 0x6c,
            Opcode::PUSH14 => 0x6d,
            Opcode::PUSH15 => 0x6e,
            Opcode::PUSH16 => 0x6f,
            Opcode::PUSH17 => 0x70,
            Opcode::PUSH18 => 0x71,
            Opcode::PUSH19 => 0x72,
            Opcode::PUSH20 => 0x73,
            Opcode::PUSH21 => 0x74,
            Opcode::PUSH22 => 0x75,
            Opcode::PUSH23 => 0x76,
            Opcode::PUSH24 => 0x77,
            Opcode::PUSH25 => 0x78,
            Opcode::PUSH26 => 0x79,
            Opcode::PUSH27 => 0x7a,
            Opcode::PUSH28 => 0x7b,
            Opcode::PUSH29 => 0x7c,
            Opcode::PUSH30 => 0x7d,
            Opcode::PUSH31 => 0x7e,
            Opcode::PUSH32 => 0x7f,

            // 80s: Duplication Operations
            Opcode::DUP1 => 0x80,
            Opcode::DUP2 => 0x81,
            Opcode::DUP3 => 0x82,
            Opcode::DUP4 => 0x83,
            Opcode::DUP5 => 0x84,
            Opcode::DUP6 => 0x85,
            Opcode::DUP7 => 0x86,
            Opcode::DUP8 => 0x87,
            Opcode::DUP9 => 0x88,
            Opcode::DUP10 => 0x89,
            Opcode::DUP11 => 0x8a,
            Opcode::DUP12 => 0x8b,
            Opcode::DUP13 => 0x8c,
            Opcode::DUP14 => 0x8d,
            Opcode::DUP15 => 0x8e,
            Opcode::DUP16 => 0x8f,

            // 90s: Exchange Operations
            Opcode::SWAP1 => 0x90,
            Opcode::SWAP2 => 0x91,
            Opcode::SWAP3 => 0x92,
            Opcode::SWAP4 => 0x93,
            Opcode::SWAP5 => 0x94,
            Opcode::SWAP6 => 0x95,
            Opcode::SWAP7 => 0x96,
            Opcode::SWAP8 => 0x97,
            Opcode::SWAP9 => 0x98,
            Opcode::SWAP10 => 0x99,
            Opcode::SWAP11 => 0x9a,
            Opcode::SWAP12 => 0x9b,
            Opcode::SWAP13 => 0x9c,
            Opcode::SWAP14 => 0x9d,
            Opcode::SWAP15 => 0x9e,
            Opcode::SWAP16 => 0x9f,

            // a0s: Logging Operations
            Opcode::LOG0 => 0xa0,
            Opcode::LOG1 => 0xa1,
            Opcode::LOG2 => 0xa2,
            Opcode::LOG3 => 0xa3,
            Opcode::LOG4 => 0xa4,

            // f0s: System Operations
            Opcode::CREATE => 0xf0,
            Opcode::CALL => 0xf1,
            Opcode::CALLCODE => 0xf2,
            Opcode::RETURN => 0xf3,
            Opcode::DELEGATECALL => 0xf4,
            Opcode::CREATE2 => 0xf5,
            Opcode::STATICCALL => 0xfa,
            Opcode::REVERT => 0xfd,
            Opcode::INVALID => 0xfe,
            Opcode::SELFDESTRUCT => 0xff,
        }
    }

    /// Get the appropriate PUSH opcode for a given byte length
    pub fn push_for_size(size: usize) -> Option<Opcode> {
        match size {
            1 => Some(Opcode::PUSH1),
            2 => Some(Opcode::PUSH2),
            3 => Some(Opcode::PUSH3),
            4 => Some(Opcode::PUSH4),
            5 => Some(Opcode::PUSH5),
            6 => Some(Opcode::PUSH6),
            7 => Some(Opcode::PUSH7),
            8 => Some(Opcode::PUSH8),
            9 => Some(Opcode::PUSH9),
            10 => Some(Opcode::PUSH10),
            11 => Some(Opcode::PUSH11),
            12 => Some(Opcode::PUSH12),
            13 => Some(Opcode::PUSH13),
            14 => Some(Opcode::PUSH14),
            15 => Some(Opcode::PUSH15),
            16 => Some(Opcode::PUSH16),
            17 => Some(Opcode::PUSH17),
            18 => Some(Opcode::PUSH18),
            19 => Some(Opcode::PUSH19),
            20 => Some(Opcode::PUSH20),
            21 => Some(Opcode::PUSH21),
            22 => Some(Opcode::PUSH22),
            23 => Some(Opcode::PUSH23),
            24 => Some(Opcode::PUSH24),
            25 => Some(Opcode::PUSH25),
            26 => Some(Opcode::PUSH26),
            27 => Some(Opcode::PUSH27),
            28 => Some(Opcode::PUSH28),
            29 => Some(Opcode::PUSH29),
            30 => Some(Opcode::PUSH30),
            31 => Some(Opcode::PUSH31),
            32 => Some(Opcode::PUSH32),
            _ => None,
        }
    }

    /// Get the appropriate DUP opcode for a given stack position (1-indexed)
    pub fn dup_for_position(pos: usize) -> Option<Opcode> {
        match pos {
            1 => Some(Opcode::DUP1),
            2 => Some(Opcode::DUP2),
            3 => Some(Opcode::DUP3),
            4 => Some(Opcode::DUP4),
            5 => Some(Opcode::DUP5),
            6 => Some(Opcode::DUP6),
            7 => Some(Opcode::DUP7),
            8 => Some(Opcode::DUP8),
            9 => Some(Opcode::DUP9),
            10 => Some(Opcode::DUP10),
            11 => Some(Opcode::DUP11),
            12 => Some(Opcode::DUP12),
            13 => Some(Opcode::DUP13),
            14 => Some(Opcode::DUP14),
            15 => Some(Opcode::DUP15),
            16 => Some(Opcode::DUP16),
            _ => None,
        }
    }

    /// Get the appropriate SWAP opcode for a given stack position (1-indexed)
    pub fn swap_for_position(pos: usize) -> Option<Opcode> {
        match pos {
            1 => Some(Opcode::SWAP1),
            2 => Some(Opcode::SWAP2),
            3 => Some(Opcode::SWAP3),
            4 => Some(Opcode::SWAP4),
            5 => Some(Opcode::SWAP5),
            6 => Some(Opcode::SWAP6),
            7 => Some(Opcode::SWAP7),
            8 => Some(Opcode::SWAP8),
            9 => Some(Opcode::SWAP9),
            10 => Some(Opcode::SWAP10),
            11 => Some(Opcode::SWAP11),
            12 => Some(Opcode::SWAP12),
            13 => Some(Opcode::SWAP13),
            14 => Some(Opcode::SWAP14),
            15 => Some(Opcode::SWAP15),
            16 => Some(Opcode::SWAP16),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_encoding() {
        assert_eq!(Opcode::ADD.encode(), 0x01);
        assert_eq!(Opcode::MUL.encode(), 0x02);
        assert_eq!(Opcode::SSTORE.encode(), 0x55);
        assert_eq!(Opcode::PUSH1.encode(), 0x60);
        assert_eq!(Opcode::DUP1.encode(), 0x80);
    }

    #[test]
    fn test_push_for_size() {
        assert_eq!(Opcode::push_for_size(1), Some(Opcode::PUSH1));
        assert_eq!(Opcode::push_for_size(32), Some(Opcode::PUSH32));
        assert_eq!(Opcode::push_for_size(33), None);
        assert_eq!(Opcode::push_for_size(0), None);
    }

    #[test]
    fn test_dup_for_position() {
        assert_eq!(Opcode::dup_for_position(1), Some(Opcode::DUP1));
        assert_eq!(Opcode::dup_for_position(16), Some(Opcode::DUP16));
        assert_eq!(Opcode::dup_for_position(17), None);
    }

    #[test]
    fn test_swap_for_position() {
        assert_eq!(Opcode::swap_for_position(1), Some(Opcode::SWAP1));
        assert_eq!(Opcode::swap_for_position(16), Some(Opcode::SWAP16));
        assert_eq!(Opcode::swap_for_position(17), None);
    }
}
