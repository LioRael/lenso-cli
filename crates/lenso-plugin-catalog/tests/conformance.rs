//! Keep the existing Marketplace cross-language vector unchanged during extraction.

use ed25519_dalek::SigningKey;
use lenso_plugin_catalog::{Snapshot, Trust, sign, verify};
use std::collections::BTreeMap;

#[test]
fn existing_wire_vector_is_verified_and_reproduced_byte_for_byte() {
    let vector: serde_json::Value = serde_json::from_str(include_str!("conformance.json")).unwrap();
    let key = SigningKey::from_bytes(&[17; 32]);
    assert_eq!(
        hex::encode(key.verifying_key().as_bytes()),
        vector["public_key_hex"]
    );
    let trust = Trust {
        catalog_id: "conformance-only".into(),
        keys: BTreeMap::from([("test-key".into(), key.verifying_key())]),
    };
    let verified = verify(
        &serde_json::to_vec(&vector["envelope"]).unwrap(),
        &trust,
        None,
        150,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(verified.snapshot()).unwrap(),
        vector["expected_payload"]
    );
    let snapshot = Snapshot::new("conformance-only".into(), 1, 100, 200, vec![]);
    let signed = sign(&snapshot, "test-key", &key).unwrap();
    assert_eq!(signed, serde_json::to_vec(&vector["envelope"]).unwrap());
}
