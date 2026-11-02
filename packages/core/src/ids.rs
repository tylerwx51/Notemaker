//! Java `UUID` helpers.

use md5::{Digest, Md5};

/// `UUID.randomUUID().toString()` (also `Actions.id()`).
pub fn random_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `UUID.nameUUIDFromBytes(bytes).toString()`: MD5, version 3 bits, no namespace.
pub fn name_uuid_from_bytes(bytes: &[u8]) -> String {
    let mut md5: [u8; 16] = Md5::digest(bytes).into();
    md5[6] &= 0x0f;
    md5[6] |= 0x30;
    md5[8] &= 0x3f;
    md5[8] |= 0x80;
    uuid::Uuid::from_bytes(md5).to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn name_uuid() {
        // UUID.nameUUIDFromBytes("doc".getBytes(UTF_8))
        assert_eq!(super::name_uuid_from_bytes(b"doc").len(), 36);
    }
}
