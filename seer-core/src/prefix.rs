use std::collections::VecDeque;

pub fn is_prefix<T: PartialEq>(prefix: &VecDeque<T>, sequence: &VecDeque<T>) -> bool {
    if prefix.len() > sequence.len() {
        return false;
    }
    prefix.iter().zip(sequence.iter()).all(|(a, b)| a == b)
}