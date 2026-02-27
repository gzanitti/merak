use std::{collections::HashMap, fmt};

use merak_ast::{
    meta::SourceRef,
    predicate::Predicate,
    types::{BaseType, Type},
};

/// Liquid variable: represents an unknown refinement
/// During inference, κ0, κ1, ... will be resolved to conjunctions of qualifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LiquidVar(pub usize);

impl fmt::Display for LiquidVar {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "κ{}", self.0)
    }
}

/// Generator for fresh liquid variables
#[derive(Debug, Clone)]
pub struct LiquidVarGenerator {
    counter: usize,
}

impl LiquidVarGenerator {
    pub fn new() -> Self {
        Self { counter: 0 }
    }

    /// Generate a new fresh liquid variable
    pub fn fresh(&mut self) -> LiquidVar {
        let var = LiquidVar(self.counter);
        self.counter += 1;
        var
    }
}

/// Template: a refined type where some refinements are unknown
///
/// Key distinction:
/// - **Liquid**: needs inference
/// - **Concrete**: needs verification
#[derive(Debug, Clone)]
pub enum Template {
    /// Template with liquid variable (unknown refinement to infer)
    ///
    /// Example: `var x = 5;` 
    /// Generated: {int | κ0} where κ0 will be resolved during solving
    Liquid {
        base: BaseType,
        binder: String,
        liquid_var: LiquidVar, // κ0, κ1, κ2, ...
        source_ref: SourceRef,
    },

    /// Concrete template (known refinement to verify)
    ///
    /// Example: `var x: {int | x > 0} = 10;`
    /// Generated: {int | x > 0} and verify that 10 satisfies x > 0
    Concrete {
        base: BaseType,
        binder: String,
        refinement: Predicate,
        source_ref: SourceRef,
    },
}

impl Template {
    pub fn from_type(ty: &Type, liquid_gen: &mut LiquidVarGenerator) -> Self {
        if ty.is_explicit_annotation() {
            Template::Concrete {
                base: ty.base.clone(),
                binder: ty.binder.clone(),
                refinement: ty.constraint.clone(),
                source_ref: ty.source_ref.clone(),
            }
        } else {
            Template::Liquid {
                base: ty.base.clone(),
                binder: ty.binder.clone(),
                liquid_var: liquid_gen.fresh(),
                source_ref: ty.source_ref.clone(),
            }
            
        }
    }

    /// Substitute non-binder variables in the refinement predicate.
    /// Used to resolve source-level names to SSA names in cross-variable
    /// references (e.g., `{v: int | v > x}` where `x` needs to become `x_0`).
    pub fn resolve_cross_refs(&mut self, source_to_ssa: &HashMap<String, String>) {
        if let Template::Concrete { binder, refinement, .. } = self {
            let mut filtered = source_to_ssa.clone();
            filtered.remove(binder);
            if !filtered.is_empty() {
                *refinement = refinement.substitute_vars(&filtered);
            }
        }
    }

    pub fn replace_binder(&mut self, new_binder: &str) {
        match self {
            Template::Concrete { binder, refinement, .. } => {
                let mut subst = HashMap::new();
                println!("BINDER NEW BINDER: {binder}, {new_binder}");
                subst.insert(binder.clone(), new_binder.to_string());

                *refinement = refinement.substitute_vars(&subst);

                *binder = new_binder.to_string();
                println!("REF {refinement}");
            },
            Template::Liquid { binder, .. } => {
                *binder = new_binder.to_string();
            },
        };
    }

    /// Get the base type
    pub fn base_type(&self) -> &BaseType {
        match self {
            Template::Liquid { base, .. } => base,
            Template::Concrete { base, .. } => base,
        }
    }

    /// Get the binder
    pub fn binder(&self) -> &str {
        match self {
            Template::Liquid { binder, .. } => binder,
            Template::Concrete { binder, .. } => binder,
        }
    }

    /// Get the source_ref
    pub fn source_ref(&self) -> &SourceRef {
        match self {
            Template::Liquid { source_ref, .. } => source_ref,
            Template::Concrete { source_ref, .. } => source_ref,
        }
    }

    /// Is this a liquid template? (needs inference)
    pub fn is_liquid(&self) -> bool {
        matches!(self, Template::Liquid { .. })
    }

    /// Is this a concrete template? (needs verification)
    pub fn is_concrete(&self) -> bool {
        matches!(self, Template::Concrete { .. })
    }

    /// Get the liquid variable if it exists
    pub fn liquid_var(&self) -> Option<LiquidVar> {
        match self {
            Template::Liquid { liquid_var, .. } => Some(*liquid_var),
            Template::Concrete { .. }  => None,
        }
    }

    /// Get the refinement if concrete
    pub fn refinement(&self) -> Option<&Predicate> {
        match self {
            Template::Concrete { refinement, .. } => Some(refinement),
            Template::Liquid { .. }  => None,
        }
    }

    // / Convert to Type by applying a liquid variable assignment
    // /
    // / - Liquid: Uses assignment from solver (warns if missing)
    // / - Concrete: Uses its known refinement
    // / - Unrefined: Intentionally produces Predicate::True (no refinement)
    // pub fn to_type(&self, assignment: &LiquidAssignment) -> Type {
    //     match self {
    //         Template::Liquid {
    //             base,
    //             binder,
    //             liquid_var,
    //             source_ref,
    //         } => {
    //             let refinement = assignment.get(*liquid_var).cloned().unwrap_or_else(|| {
    //                 // This SHOULDN'T happen - means inference failed
    //                 eprintln!("Warning: No assignment for {}, using True", liquid_var);
    //                 Predicate::True(NodeId::new(0), source_ref.clone())
    //             });
    //             Type {
    //                 base: base.clone(),
    //                 binder: binder.clone(),
    //                 constraint: refinement,
    //                 source_ref: source_ref.clone(),
    //             }
    //         }

    //         Template::Concrete {
    //             base,
    //             binder,
    //             refinement,
    //             source_ref,
    //         } => Type {
    //             base: base.clone(),
    //             binder: binder.clone(),
    //             constraint: refinement.clone(),
    //             source_ref: source_ref.clone(),
    //         },
    //     }
    // }
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Template::Liquid {
                base,
                binder,
                liquid_var,
                ..
            } => {
                if binder == "ν" {
                    write!(f, "{{{} | {}}}", base, liquid_var)
                } else {
                    write!(f, "{{{}: {} | {}}}", binder, base, liquid_var)
                }
            }
            Template::Concrete {
                base,
                binder,
                refinement,
                ..
            } => {
                if binder == "ν" {
                    write!(f, "{{{} | {}}}", base, refinement)
                } else {
                    write!(f, "{{{}: {} | {}}}", binder, base, refinement)
                }
            }
        }
    }
}

/// Assignment of liquid variables to concrete predicates
///
/// Example: {κ0 → (0 ≤ V ∧ V < 100), κ1 → (10 ≤ V ∧ V < 50)}
#[derive(Debug, Clone)]
pub struct LiquidAssignment {
    assignments: HashMap<LiquidVar, Predicate>,
}

impl LiquidAssignment {
    pub fn new() -> Self {
        Self {
            assignments: HashMap::new(),
        }
    }

    /// Assign a predicate to a liquid variable
    pub fn assign(&mut self, var: LiquidVar, predicate: Predicate) {
        self.assignments.insert(var, predicate);
    }

    /// Get the assignment for a liquid variable
    pub fn get(&self, var: LiquidVar) -> Option<&Predicate> {
        self.assignments.get(&var)
    }

    /// Does this have an assignment for this variable?
    pub fn has(&self, var: LiquidVar) -> bool {
        self.assignments.contains_key(&var)
    }

    /// Iterate over all assignments
    pub fn iter(&self) -> impl Iterator<Item = (&LiquidVar, &Predicate)> {
        self.assignments.iter()
    }

    /// Number of assignments
    pub fn len(&self) -> usize {
        self.assignments.len()
    }

    /// Is empty?
    pub fn is_empty(&self) -> bool {
        self.assignments.is_empty()
    }
}
