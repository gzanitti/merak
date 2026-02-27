# Codegen Integration Tests

Tests de integración end-to-end que compilan código Merak hasta bytecode EVM y lo ejecutan con revm.

## Estructura

- **`common.rs`** - Utilidades compartidas:
  - `compile_from_source()` - Compila código Merak a bytecode
  - `TestRuntime` - Runtime de prueba con revm para ejecutar bytecode
  - `CallResult` - Decodifica resultados (uint256, bool, address)
  - Funciones helper para encoding de calldata y selectores

- **`integration.rs`** - Suite de tests de integración

## Tests Implementados

### ✅ Tests que PASAN (3/8)

1. **`test_simple_return_constant`** - Devuelve constante 42
   - Contract con `entrypoint` que retorna valor constante
   - Bytecode: 11 bytes
   - ✅ Funciona correctamente

2. **`test_simple_arithmetic`** - Suma 10 + 32 = 42
   - Contract con función `external` y operación ADD
   - Dispatcher funciona correctamente
   - Bytecode: 37 bytes
   - ✅ Funciona correctamente

3. **`test_multiple_functions`** - Múltiples funciones con dispatcher
   - Contract con 3 funciones: get_ten(), get_twenty(), sum_both()
   - El dispatcher EVM selecciona correctamente la función según selector
   - ✅ Todas las funciones se llaman correctamente

### ⚠️ Tests que FALLAN (5/8)

4. **`test_arithmetic_operations`** - Operaciones múltiples
   - Expression: `(100 - 50) * 2` debería ser 100
   - **Problema**: Devuelve valor incorrecto (número muy grande, complemento a 2)
   - **Causa**: El bytecode generado no respeta el orden de evaluación de expresiones
   - El bytecode genera: `PUSH 100, PUSH 50, PUSH 2, SUB, MUL`
   - Pero SUB opera sobre los dos últimos valores (50 - 2 = 48)
   - **Solución requerida**: Sistema de gestión de stack que respete dependencias entre operaciones

5. **`test_boolean_operations`** - Operaciones booleanas
   - Funciones: always_true(), always_false(), compare_numbers()
   - **Problema**: always_false() devuelve true
   - **Causa**: Similar al anterior, problemas con orden de evaluación o dispatcher

6. **`test_conditional_branches`** - If/else con parámetros
   - Funciones: max(a, b), is_positive(x)
   - **Problema**: Los parámetros de función no se leen del calldata
   - **Solución requerida**: Implementar carga de parámetros desde calldata

7. **`test_storage_write_and_read`** - Operaciones de storage
   - Contract con variable de estado `counter`
   - Funciones: set_counter(), get_counter()
   - **Problema**: StackUnderflow durante ejecución
   - **Causa**: Las instrucciones de storage (SLOAD/SSTORE) no están funcionando correctamente

8. **`test_storage_increment`** - Incremento de storage
   - Contract que incrementa una variable de estado
   - **Problema**: Similar al anterior, issues con storage operations

## Arquitectura del Dispatcher

El dispatcher EVM implementado funciona así:

```
1. CALLDATALOAD 0     // Cargar primeros 32 bytes del calldata
2. SHR 224            // Shift right 224 bits → primeros 4 bytes (selector)
3. Para cada función:
   a. DUP1            // Duplicar selector
   b. PUSH <selector> // Empujar selector esperado
   c. EQ              // Comparar
   d. PUSH <label>    // Dirección de la función
   e. JUMPI           // Saltar si igual
4. REVERT             // Si no hay match, revertir
```

## Problemas Identificados

### 1. Gestión de Stack EVM

**Problema**: El codegen actual traduce instrucciones SSA linealmente sin considerar el estado del stack EVM.

**Ejemplo**:
```merak
var a: int = 100;
var b: int = 50;
var c: int = 2;
var result: int = (a - b) * c;  // Debería ser 100
```

**SSA IR generado**:
```
%a = Copy 100
%b = Copy 50
%c = Copy 2
%temp = BinaryOp SUB %a %b
%result = BinaryOp MUL %temp %c
```

**Bytecode generado** (INCORRECTO):
```
PUSH 100  // a
PUSH 50   // b
PUSH 2    // c
SUB       // 50 - 2 = 48 ❌
MUL       // 100 * 48 = 4800 ❌
```

**Bytecode esperado** (CORRECTO):
```
PUSH 100
PUSH 50
SUB       // 100 - 50 = 50
PUSH 2
MUL       // 50 * 2 = 100 ✅
```

**Solución necesaria**:
- Implementar análisis de liveness y uso de registros
- Generar código que respete dependencias de datos
- Usar stack slots específicos o memoria para valores intermedios

### 2. Parámetros de Función

**Problema**: Los parámetros de función no se cargan del calldata.

**Ejemplo**:
```merak
external function max(a: int, b: int) -> int
```

**Solución necesaria**:
- Leer parámetros desde calldata después del selector
- CALLDATALOAD desde offsets correctos (4, 36, 68, ...)
- Asignar a los registros/slots correspondientes

### 3. Storage Operations

**Problema**: SLOAD/SSTORE causan StackUnderflow.

**Posibles causas**:
- Stack no preparado correctamente antes de SLOAD/SSTORE
- Orden incorrecto de operandos
- Falta limpieza del stack después de operaciones

**Solución necesaria**:
- Revisar codegen de storage_ops.rs
- Verificar que el stack tenga los valores correctos antes de cada operación

## Cómo Ejecutar

```bash
# Todos los tests
cargo test -p merak-codegen --test integration

# Test específico
cargo test -p merak-codegen --test integration test_simple_arithmetic -- --nocapture

# Ver bytecode generado
cargo test -p merak-codegen --test integration -- --nocapture 2>&1 | grep "Bytecode"
```

## Próximos Pasos

### Prioridad Alta
1. **Arreglar gestión de stack** - Crítico para que aritmética funcione
2. **Implementar lectura de parámetros** - Necesario para funciones con argumentos
3. **Verificar storage operations** - Debugging de SLOAD/SSTORE

### Prioridad Media
4. Optimización de bytecode (eliminar PUSH/POP innecesarios)
5. Manejo de tipos más complejos (structs, arrays)
6. Soporte para llamadas a otras funciones/contratos

### Prioridad Baja
7. Optimizaciones de gas
8. Metadata y debugging info
9. Soporte para eventos y logs

## Dependencias

- `revm = "14"` - EVM runtime para ejecutar bytecode
- `primitive-types = "0.13.1"` - U256, H160, etc.
- `tiny-keccak = "2.0"` - Para calcular selectores de función
