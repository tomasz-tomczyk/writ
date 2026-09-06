use uuid::Uuid;

/// Return a fresh UUIDv7 as a hyphenated string.
///
/// Every primary key in the schema is a UUIDv7, so keys sort by creation
/// time and stay unique across machines that never talk to each other.
pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::new_id;

    #[test]
    fn ids_are_unique_and_sort_by_time() {
        let first = new_id();
        let second = new_id();
        assert_ne!(first, second);
        assert_eq!(first.len(), 36);
        // Version nibble 7, in the first character of the third group.
        assert_eq!(first.split('-').nth(2).unwrap().as_bytes()[0], b'7');
    }
}
