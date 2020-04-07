use super::identifier::*;
use crate::analysis::abstract_domain::*;
use crate::bil::*;
use crate::prelude::*;
use std::collections::BTreeMap;

/// An abstract value representing either a pointer or a constant value.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Data {
    Top(BitSize),
    Pointer(PointerDomain),
    Value(BitvectorDomain),
}

impl Data {
    pub fn bitvector(bitv: Bitvector) -> Data {
        Data::Value(BitvectorDomain::Value(bitv))
    }
}

/// An abstract value representing a pointer given as a map from an abstract identifier
/// to the offset in the pointed to object.
///
/// The map should never be empty. If the map contains more than one key,
/// it indicates that the pointer may point to any of the contained objects.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct PointerDomain(BTreeMap<AbstractIdentifier, BitvectorDomain>);

impl PointerDomain {
    pub fn new(target: AbstractIdentifier, offset: BitvectorDomain) -> PointerDomain {
        let mut map = BTreeMap::new();
        map.insert(target, offset);
        PointerDomain(map)
    }

    /// get the bitsize of the pointer
    pub fn bitsize(&self) -> BitSize {
        let some_elem = self.0.values().next().unwrap();
        some_elem.bitsize()
    }

    pub fn merge(&self, other: &PointerDomain) -> PointerDomain {
        let mut merged_map = self.0.clone();
        for (location, offset) in other.0.iter() {
            if merged_map.contains_key(location) {
                merged_map.insert(location.clone(), merged_map[location].merge(offset));
            } else {
                merged_map.insert(location.clone(), offset.clone());
            }
        }
        PointerDomain(merged_map)
    }

    /// add a value to the offset
    pub fn add_to_offset(&self, value: &BitvectorDomain) -> PointerDomain {
        let mut result = self.clone();
        for offset in result.0.values_mut() {
            *offset = offset.bin_op(BinOpType::PLUS, value);
        }
        result
    }

    /// subtract a value from the offset
    pub fn sub_from_offset(&self, value: &BitvectorDomain) -> PointerDomain {
        let mut result = self.clone();
        for offset in result.0.values_mut() {
            *offset = offset.bin_op(BinOpType::MINUS, value);
        }
        result
    }

    /// Get an iterator over all possible abstract targets (together with the offset in the target) the pointer may point to.
    pub fn iter_targets(
        &self,
    ) -> std::collections::btree_map::Iter<AbstractIdentifier, BitvectorDomain> {
        self.0.iter()
    }
}

impl ValueDomain for Data {
    fn bitsize(&self) -> BitSize {
        use Data::*;
        match self {
            Top(size) => *size,
            Pointer(pointer) => pointer.bitsize(),
            Value(bitvec) => bitvec.bitsize(),
        }
    }

    fn new_top(bitsize: BitSize) -> Data {
        Data::Top(bitsize)
    }

    /// Compute the (abstract) result of a binary operation
    fn bin_op(&self, op: BinOpType, rhs: &Self) -> Self {
        use BinOpType::*;
        use Data::*;
        match (self, op, rhs) {
            (Value(left), _, Value(right)) => Value(left.bin_op(op, right)),
            (Pointer(pointer), PLUS, Value(value)) | (Value(value), PLUS, Pointer(pointer)) => {
                Pointer(pointer.add_to_offset(value))
            }
            (Pointer(pointer), MINUS, Value(value)) => Pointer(pointer.sub_from_offset(value)),
            // TODO: AND and OR binops may be used to compute pointers when alignment information about the pointer is known.
            _ => ValueDomain::new_top(self.bitsize()),
        }
    }

    /// Compute the (abstract) result of a unary operation
    fn un_op(&self, op: UnOpType) -> Self {
        if let Data::Value(value) = self {
            Data::Value(value.un_op(op))
        } else {
            Data::new_top(self.bitsize())
        }
    }

    /// extract a sub-bitvector
    fn extract(&self, low_bit: BitSize, high_bit: BitSize) -> Self {
        if let Data::Value(value) = self {
            Data::Value(value.extract(low_bit, high_bit))
        } else {
            Data::new_top(self.bitsize())
        }
    }

    /// Extend a bitvector using the given cast type
    fn cast(&self, kind: CastType, width: BitSize) -> Self {
        if let Data::Value(value) = self {
            Data::Value(value.cast(kind, width))
        } else {
            Data::new_top(width)
        }
    }

    /// Concatenate two bitvectors
    fn concat(&self, other: &Self) -> Self {
        if let (Data::Value(upper_bits), Data::Value(lower_bits)) = (self, other) {
            Data::Value(upper_bits.concat(lower_bits))
        } else {
            Data::new_top(self.bitsize() + other.bitsize())
        }
    }
}

impl AbstractDomain for Data {
    fn top(&self) -> Self {
        Data::Top(self.bitsize())
    }

    fn merge(&self, other: &Self) -> Self {
        use Data::*;
        match (self, other) {
            (Top(bitsize), _) | (_, Top(bitsize)) => Top(*bitsize),
            (Pointer(pointer1), Pointer(pointer2)) => Pointer(pointer1.merge(pointer2)),
            (Value(val1), Value(val2)) => Value(val1.merge(val2)),
            (Pointer(_), Value(_)) | (Value(_), Pointer(_)) => Top(self.bitsize()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bv(value: i64) -> BitvectorDomain {
        BitvectorDomain::Value(Bitvector::from_i64(value))
    }

    fn new_id(name: String) -> AbstractIdentifier {
        AbstractIdentifier::new(Tid::new("time0"), AbstractLocation::Register(name, 64))
    }

    fn new_pointer_domain(location: String, offset: i64) -> PointerDomain {
        let id = new_id(location);
        PointerDomain::new(id, bv(offset))
    }

    fn new_pointer(location: String, offset: i64) -> Data {
        Data::Pointer(new_pointer_domain(location, offset))
    }

    fn new_value(value: i64) -> Data {
        Data::Value(bv(value))
    }

    #[test]
    fn data_abstract_domain() {
        let pointer = new_pointer("Rax".into(), 0);
        let data = new_value(42);
        assert_eq!(pointer.merge(&pointer), pointer);
        assert_eq!(pointer.merge(&data), Data::new_top(64));
        assert_eq!(
            data.merge(&new_value(41)),
            Data::Value(BitvectorDomain::new_top(64))
        );

        let other_pointer = new_pointer("Rbx".into(), 0);
        match pointer.merge(&other_pointer) {
            Data::Pointer(_) => (),
            _ => panic!(),
        }
    }

    #[test]
    fn data_value_domain() {
        use crate::bil::BinOpType::*;
        let data = new_value(42);
        assert_eq!(data.bitsize(), 64);

        let three = new_value(3);
        let pointer = new_pointer("Rax".into(), 0);
        assert_eq!(data.bin_op(PLUS, &three), new_value(45));
        assert_eq!(pointer.bin_op(PLUS, &three), new_pointer("Rax".into(), 3));
        assert_eq!(three.un_op(crate::bil::UnOpType::NEG), new_value(-3));

        assert_eq!(three.extract(0, 31), Data::Value(BitvectorDomain::Value(Bitvector::from_i32(3))));

        assert_eq!(data.cast(crate::bil::CastType::SIGNED, 128).bitsize(), 128);

        let one = Data::Value(BitvectorDomain::Value(Bitvector::from_i32(1)));
        let two = Data::Value(BitvectorDomain::Value(Bitvector::from_i32(2)));
        let concat = new_value((1 << 32) + 2);
        assert_eq!(one.concat(&two), concat);
    }

    #[test]
    fn pointer_domain() {
        let pointer = new_pointer_domain("Rax".into(), 0);
        let offset = bv(3);

        let pointer_plus = new_pointer_domain("Rax".into(), 3);
        let pointer_minus = new_pointer_domain("Rax".into(), -3);
        assert_eq!(pointer.add_to_offset(&offset), pointer_plus);
        assert_eq!(pointer.sub_from_offset(&offset), pointer_minus);

        let other_pointer = new_pointer_domain("Rbx".into(), 5);
        let merged = pointer.merge(&other_pointer);
        assert_eq!(merged.0.len(), 2);
        assert_eq!(merged.0.get(&new_id("Rax".into())), Some(&bv(0)));
        assert_eq!(merged.0.get(&new_id("Rbx".into())), Some(&bv(5)));
    }
}