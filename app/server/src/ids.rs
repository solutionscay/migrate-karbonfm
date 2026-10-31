//! New primary keys: random UUIDs (version 4), as FileMaker's `Get(UUID)` auto-enter gives the source tables.

/// A new lowercase UUID v4, like `3f2b8c1e-9a4d-4c7b-8e21-5d0f6a9b7c34`.
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the OS random source is available");
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

#[cfg(test)]
mod tests {
    #[test]
    fn uuid_v4_shape_and_unique() {
        let a = super::new_id();
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(a, super::new_id());
    }
}
