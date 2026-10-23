#![allow(dead_code)]

use super::solve::BitSet;

#[test]
fn bitset_tracks_word_boundaries_in_order() {
    let mut set = BitSet::new(130);
    assert_eq!(set.iter().collect::<Vec<_>>(), Vec::<usize>::new());
    for index in [129, 64, 0, 63, 65, 64] {
        set.insert(index);
    }
    assert_eq!(set.iter().collect::<Vec<_>>(), vec![0, 63, 64, 65, 129]);
    for index in [0, 63, 64, 65, 129] {
        assert!(set.contains(index));
    }
    for index in [1, 62, 66, 128, 192] {
        assert!(!set.contains(index));
    }
}

#[test]
fn bitset_intersection_handles_empty_and_different_capacities() {
    let mut left = BitSet::new(130);
    let mut right = BitSet::new(65);
    for index in [0, 63, 64, 129] {
        left.insert(index);
    }
    for index in [1, 63, 64] {
        right.insert(index);
    }
    assert_eq!((&left & &right).iter().collect::<Vec<_>>(), vec![63, 64]);
    assert_eq!((&right & &left).iter().collect::<Vec<_>>(), vec![63, 64]);
    assert!((&left & &BitSet::new(0)).iter().next().is_none());
    assert!(!BitSet::new(0).contains(0));
}
