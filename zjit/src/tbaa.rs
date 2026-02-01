//! Type-Based Alias Analysis (TBAA) for memory optimization
//!
//! This module implements a simple TBAA system that allows the optimizer to
//! understand which memory operations may alias each other based on the types
//! of Ruby objects being accessed.
//!
//! The key insight is that writes to different object types (e.g., writing to
//! an Array vs writing to a Hash) cannot alias each other, allowing the compiler
//! to reorder or eliminate redundant memory operations.

use crate::hir::{Insn, InsnId, Function};
use crate::hir_type::{Type, types};
use std::collections::HashMap;

/// Alias classes represent categories of memory locations that may alias.
/// Two memory operations with different alias classes are guaranteed not to alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AliasClass {
    /// Instance variables of Array objects
    ArrayIvar,
    /// Instance variables of Hash objects
    HashIvar,
    /// Instance variables of String objects
    StringIvar,
    /// Instance variables of Integer objects (though rare)
    IntegerIvar,
    /// Instance variables of Float objects
    FloatIvar,
    /// Instance variables of Symbol objects
    SymbolIvar,
    /// Instance variables of Range objects
    RangeIvar,
    /// Instance variables of Regexp objects
    RegexpIvar,
    /// Instance variables of any other object type
    OtherIvar,
    /// Global variables
    GlobalVar,
    /// Local variables on the heap or in parent scopes
    LocalVar,
    /// Unknown or mixed type - may alias with anything
    Unknown,
}

impl AliasClass {
    /// Determine if two alias classes may alias each other.
    /// Returns true if the two classes might refer to the same memory location.
    pub fn may_alias(&self, other: &AliasClass) -> bool {
        match (self, other) {
            // Same alias class always aliases
            (a, b) if a == b => true,
            // Unknown aliases with everything
            (AliasClass::Unknown, _) | (_, AliasClass::Unknown) => true,
            // Different concrete classes don't alias
            _ => false,
        }
    }

    /// Get the alias class for an instance variable access based on the object's type.
    pub fn from_ivar_type(obj_type: &Type) -> AliasClass {
        // Use is_subtype to check what type the object could be
        if obj_type.is_subtype(types::Array) {
            AliasClass::ArrayIvar
        } else if obj_type.is_subtype(types::Hash) {
            AliasClass::HashIvar
        } else if obj_type.is_subtype(types::String) {
            AliasClass::StringIvar
        } else if obj_type.is_subtype(types::Integer) {
            // Integers (Fixnum and Bignum) are immediate values and don't typically have ivars,
            // but if they do, use specific classes
            AliasClass::IntegerIvar
        } else if obj_type.is_subtype(types::Float) {
            AliasClass::FloatIvar
        } else if obj_type.is_subtype(types::Symbol) {
            AliasClass::SymbolIvar
        } else if obj_type.is_subtype(types::Range) {
            AliasClass::RangeIvar
        } else if obj_type.is_subtype(types::Regexp) {
            AliasClass::RegexpIvar
        } else if obj_type.is_subtype(types::HeapObject) {
            // Generic heap object, could be anything
            AliasClass::OtherIvar
        } else {
            // Unknown type - conservatively assume may alias with anything
            AliasClass::Unknown
        }
    }
}

/// Represents a memory location that can be loaded from or stored to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MemoryLocation {
    /// Instance variable access: (object_id, ivar_id, alias_class)
    InstanceVariable(InsnId, u64, AliasClass),
    /// Global variable access: (global_var_id)
    GlobalVariable(u64),
    /// Local variable access: (level, ep_offset)
    LocalVariable(u32, u32),
}

impl MemoryLocation {
    /// Check if this location may alias with another location.
    pub fn may_alias(&self, other: &MemoryLocation) -> bool {
        match (self, other) {
            // Same exact location always aliases
            (a, b) if a == b => true,
            
            // Instance variables: check object and alias class
            (MemoryLocation::InstanceVariable(obj1, id1, class1),
             MemoryLocation::InstanceVariable(obj2, id2, class2)) => {
                // If different objects, check if alias classes overlap
                if obj1 != obj2 {
                    class1.may_alias(class2)
                } else {
                    // Same object: only aliases if same ivar or unknown class
                    id1 == id2 || class1.may_alias(&AliasClass::Unknown) || class2.may_alias(&AliasClass::Unknown)
                }
            }
            
            // Global variables: same ID means same location
            (MemoryLocation::GlobalVariable(id1), MemoryLocation::GlobalVariable(id2)) => {
                id1 == id2
            }
            
            // Local variables: same level and offset means same location
            (MemoryLocation::LocalVariable(l1, o1), MemoryLocation::LocalVariable(l2, o2)) => {
                l1 == l2 && o1 == o2
            }
            
            // Different location types don't alias
            _ => false,
        }
    }
}

/// Tracks memory operations within a function for optimization.
pub struct MemoryOpTracker {
    /// Map from instruction ID to its memory location (for loads and stores)
    locations: HashMap<InsnId, MemoryLocation>,
    
    /// Map from instruction ID to the type of the object being accessed
    object_types: HashMap<InsnId, Type>,
}

impl MemoryOpTracker {
    /// Create a new memory operation tracker.
    pub fn new() -> Self {
        MemoryOpTracker {
            locations: HashMap::new(),
            object_types: HashMap::new(),
        }
    }

    /// Analyze a function and extract memory locations from load/store operations.
    pub fn analyze(&mut self, func: &Function) {
        for block_id in func.rpo() {
            let block = func.block(block_id);
            for insn_id in block.insns() {
                let insn = func.find(*insn_id);
                self.analyze_insn(func, *insn_id, &insn);
            }
        }
    }

    fn analyze_insn(&mut self, func: &Function, insn_id: InsnId, insn: &Insn) {
        match insn {
            Insn::GetIvar { self_val, id, .. } => {
                // Get the type of the object
                let obj_type = func.type_of(*self_val);
                let alias_class = AliasClass::from_ivar_type(&obj_type);
                
                self.locations.insert(
                    insn_id,
                    MemoryLocation::InstanceVariable(*self_val, id.0, alias_class)
                );
                self.object_types.insert(insn_id, obj_type);
            }
            
            Insn::SetIvar { self_val, id, .. } => {
                let obj_type = func.type_of(*self_val);
                let alias_class = AliasClass::from_ivar_type(&obj_type);
                
                self.locations.insert(
                    insn_id,
                    MemoryLocation::InstanceVariable(*self_val, id.0, alias_class)
                );
                self.object_types.insert(insn_id, obj_type);
            }
            
            Insn::GetGlobal { id, .. } => {
                self.locations.insert(
                    insn_id,
                    MemoryLocation::GlobalVariable(id.0)
                );
            }
            
            Insn::SetGlobal { id, .. } => {
                self.locations.insert(
                    insn_id,
                    MemoryLocation::GlobalVariable(id.0)
                );
            }
            
            Insn::GetLocal { level, ep_offset } => {
                self.locations.insert(
                    insn_id,
                    MemoryLocation::LocalVariable(*level, *ep_offset)
                );
            }
            
            Insn::SetLocal { level, ep_offset, .. } => {
                self.locations.insert(
                    insn_id,
                    MemoryLocation::LocalVariable(*level, *ep_offset)
                );
            }
            
            _ => {}
        }
    }

    /// Get the memory location for an instruction, if it's a memory operation.
    pub fn get_location(&self, insn_id: InsnId) -> Option<&MemoryLocation> {
        self.locations.get(&insn_id)
    }

    /// Check if two instructions may alias each other.
    pub fn may_alias(&self, insn1: InsnId, insn2: InsnId) -> bool {
        match (self.locations.get(&insn1), self.locations.get(&insn2)) {
            (Some(loc1), Some(loc2)) => loc1.may_alias(loc2),
            // If we don't have location info, conservatively assume they may alias
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alias_class_same_type() {
        let array1 = AliasClass::ArrayIvar;
        let array2 = AliasClass::ArrayIvar;
        assert!(array1.may_alias(&array2));
    }

    #[test]
    fn test_alias_class_different_types() {
        let array = AliasClass::ArrayIvar;
        let hash = AliasClass::HashIvar;
        assert!(!array.may_alias(&hash));
    }

    #[test]
    fn test_alias_class_unknown() {
        let array = AliasClass::ArrayIvar;
        let unknown = AliasClass::Unknown;
        assert!(array.may_alias(&unknown));
        assert!(unknown.may_alias(&array));
    }

    #[test]
    fn test_memory_location_same_object_same_ivar() {
        let obj = InsnId(0);
        let loc1 = MemoryLocation::InstanceVariable(obj, 1, AliasClass::ArrayIvar);
        let loc2 = MemoryLocation::InstanceVariable(obj, 1, AliasClass::ArrayIvar);
        assert!(loc1.may_alias(&loc2));
    }

    #[test]
    fn test_memory_location_same_object_different_ivar() {
        let obj = InsnId(0);
        let loc1 = MemoryLocation::InstanceVariable(obj, 1, AliasClass::ArrayIvar);
        let loc2 = MemoryLocation::InstanceVariable(obj, 2, AliasClass::ArrayIvar);
        assert!(!loc1.may_alias(&loc2));
    }

    #[test]
    fn test_memory_location_different_objects_different_types() {
        let obj1 = InsnId(0);
        let obj2 = InsnId(1);
        let loc1 = MemoryLocation::InstanceVariable(obj1, 1, AliasClass::ArrayIvar);
        let loc2 = MemoryLocation::InstanceVariable(obj2, 1, AliasClass::HashIvar);
        // Different objects with different types don't alias
        assert!(!loc1.may_alias(&loc2));
    }

    #[test]
    fn test_memory_location_different_objects_same_type() {
        let obj1 = InsnId(0);
        let obj2 = InsnId(1);
        let loc1 = MemoryLocation::InstanceVariable(obj1, 1, AliasClass::ArrayIvar);
        let loc2 = MemoryLocation::InstanceVariable(obj2, 1, AliasClass::ArrayIvar);
        // Different objects but same type may alias
        assert!(loc1.may_alias(&loc2));
    }

    #[test]
    fn test_global_variables() {
        let loc1 = MemoryLocation::GlobalVariable(1);
        let loc2 = MemoryLocation::GlobalVariable(1);
        let loc3 = MemoryLocation::GlobalVariable(2);
        assert!(loc1.may_alias(&loc2));
        assert!(!loc1.may_alias(&loc3));
    }

    #[test]
    fn test_local_variables() {
        let loc1 = MemoryLocation::LocalVariable(0, 1);
        let loc2 = MemoryLocation::LocalVariable(0, 1);
        let loc3 = MemoryLocation::LocalVariable(0, 2);
        let loc4 = MemoryLocation::LocalVariable(1, 1);
        assert!(loc1.may_alias(&loc2));
        assert!(!loc1.may_alias(&loc3));
        assert!(!loc1.may_alias(&loc4));
    }

    #[test]
    fn test_cross_type_no_alias() {
        let ivar = MemoryLocation::InstanceVariable(InsnId(0), 1, AliasClass::ArrayIvar);
        let global = MemoryLocation::GlobalVariable(1);
        let local = MemoryLocation::LocalVariable(0, 1);
        
        assert!(!ivar.may_alias(&global));
        assert!(!ivar.may_alias(&local));
        assert!(!global.may_alias(&local));
    }
}
