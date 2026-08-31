use crate::extraction_plan::first_duplicate_index;
use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
};

#[test]
fn unique_key_validation_hashes_each_key_once_after_reserving() {
    const KEY_COUNT: usize = 4_096;

    let hash_calls = Rc::new(Cell::new(0_usize));
    let keys = (0..KEY_COUNT)
        .map(|value| CountedKey {
            value: u64::try_from(value).unwrap(),
            hash_calls: Rc::clone(&hash_calls),
        })
        .collect::<Vec<_>>();

    assert_eq!(first_duplicate_index(keys.into_iter()).unwrap(), None);
    assert_eq!(hash_calls.get(), KEY_COUNT);
}

#[test]
fn validation_stops_at_the_first_repeated_key_in_input_order() {
    let hash_calls = Rc::new(Cell::new(0_usize));
    let keys = [1_u64, 2, 3, 2, 1]
        .into_iter()
        .map(|value| CountedKey {
            value,
            hash_calls: Rc::clone(&hash_calls),
        });

    assert_eq!(first_duplicate_index(keys).unwrap(), Some(3));
    assert_eq!(hash_calls.get(), 4);
}

#[test]
fn impossible_auxiliary_capacity_returns_a_typed_reservation_failure() {
    assert!(first_duplicate_index(ImpossibleCapacity).is_err());
}

#[derive(Debug)]
struct CountedKey {
    value: u64,
    hash_calls: Rc<Cell<usize>>,
}

impl PartialEq for CountedKey {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for CountedKey {}

impl Hash for CountedKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash_calls.set(self.hash_calls.get() + 1);
        self.value.hash(state);
    }
}

struct ImpossibleCapacity;

impl Iterator for ImpossibleCapacity {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, Some(usize::MAX))
    }
}

impl ExactSizeIterator for ImpossibleCapacity {
    fn len(&self) -> usize {
        usize::MAX
    }
}
