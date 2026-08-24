//! Runs dragonfruit's golden-vector verify under the TRIMMED feature set (the
//! config the firmware links: ed25519-dalek/sha2 default-features=false), on the
//! host. The gap this closes: the in-crate `serde_vectors.rs` KATs run under the dev-dep-
//! re-unified config and so do NOT exercise this; this standalone crate does.
//!
//! The values are the same golden cascade pinned in
//! `crates/dragonfruit/tests/serde_vectors.rs::signed_bundle_golden_vector`
//! (seeds [1;32]/[2;32], artifact "dragonfruit golden artifact"); a divergence here
//! vs there means the trimmed and unified configs disagree — exactly the regression
//! this gate exists to catch.

use dragonfruit::{
    verify_bundle_no_window, verify_bundle_over_bytes, BundleFile, Purpose, VerifyError,
    BUNDLE_FILE_LEN,
};

const ROOT_PUB_HEX: &str = "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
const ARTIFACT: &[u8] = b"dragonfruit golden artifact";
                                                                                        
                                                                                            
const ARTIFACT_HASH_HEX: &str = "f8fa05fd8b8b48df05cc3a5a0a9d0d40f96a4e4849f0c43a031f00dabe10bea1";
                                                                                      
const BUNDLE_HEX: &str = concat!(
    "01018139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394",
    "0000000000000003e800000000000007d00000000000000001",
    "329a99b0ccd7c0dd6154af0e70c6423f8fd3963fa8ce67f374738aa25eea270e",
    "823eb457d24e6fb3e8dcf0a8ea5820a807fd0c913128222d1f7f15c4b52b7b0c",
    "0201f8fa05fd8b8b48df05cc3a5a0a9d0d40f96a4e4849f0c43a031f00dabe10bea1",
    "001884134ef96db8ca7a1710349a29be345c576d1d477f5d42abcb42412d8bdddb",
    "68d20066cdede9a2b4ac0822d77b32a634ada11ae279f0d2c39a92157efbad21",
    "ff2f9523530dc43882ab5ef90864a2c6fb108683fd467fffce1301e62f1cd10c",
);

fn root_pub() -> [u8; 32] {
    hex::decode(ROOT_PUB_HEX).unwrap().try_into().unwrap()
}

#[test]
fn golden_bundle_verifies_under_trimmed_dragonfruit() {
    let bytes = hex::decode(BUNDLE_HEX).expect("golden hex");
    assert_eq!(bytes.len(), BUNDLE_FILE_LEN);
    let bf = BundleFile::from_bytes(&bytes).expect("254-byte parse");
                                                                    
    verify_bundle_over_bytes(&bf.as_bundle(), &root_pub(), 1_500, ARTIFACT, Purpose::Img)
        .expect("golden bundle must verify under the trimmed config");
}

#[test]
fn tampered_bundle_rejects_under_trimmed_dragonfruit() {
    let mut bytes = hex::decode(BUNDLE_HEX).unwrap();
    bytes[200] ^= 0x01;                                                    
    let bf = BundleFile::from_bytes(&bytes).unwrap();
    let e = verify_bundle_over_bytes(&bf.as_bundle(), &root_pub(), 1_500, ARTIFACT, Purpose::Img)
        .unwrap_err();
    assert!(matches!(e, VerifyError::BadSignature));
}

#[test]
fn wrong_artifact_rejects_under_trimmed_dragonfruit() {
    let bytes = hex::decode(BUNDLE_HEX).unwrap();
    let bf = BundleFile::from_bytes(&bytes).unwrap();
    let e = verify_bundle_over_bytes(
        &bf.as_bundle(),
        &root_pub(),
        1_500,
        b"different bytes",
        Purpose::Img,
    )
    .unwrap_err();
    assert_eq!(e, VerifyError::ArtifactHashMismatch);
}

fn artifact_hash() -> [u8; 32] {
    hex::decode(ARTIFACT_HASH_HEX).unwrap().try_into().unwrap()
}

#[test]
fn golden_bundle_verifies_windowless_under_trimmed_dragonfruit() {
                                                                                              
                                                                                               
                                                                 
    let bytes = hex::decode(BUNDLE_HEX).expect("golden hex");
    let bf = BundleFile::from_bytes(&bytes).expect("254-byte parse");
                                                                                              
    assert_eq!(
        verify_bundle_over_bytes(&bf.as_bundle(), &root_pub(), 3_000, ARTIFACT, Purpose::Img)
            .unwrap_err(),
        VerifyError::WindowExpired
    );
                                                                                             
                                                                                
    let wua = verify_bundle_no_window(
        &bf.as_bundle(),
        &root_pub(),
        &artifact_hash(),
        Purpose::Img,
        0,
    )
    .expect("windowless golden must verify under the trimmed config");
    assert_eq!(wua.purpose(), Purpose::Img);
    assert_eq!(wua.not_after(), 2_000);                                  
}
