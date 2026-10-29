// Offline public synthetic vectors only; never keys, Tor clients or publication.
#[cfg(feature = "tor")]
fn main() {
    use std::io::Read;
    use tor_hscrypto::pk::HsId;
    let mut input = String::new();
    std::io::stdin()
        .take(4096)
        .read_to_string(&mut input)
        .unwrap();
    let vectors: serde_json::Value = serde_json::from_str(&input).unwrap();
    let vectors = vectors.as_array().unwrap();
    assert_eq!(vectors.len(), 3);
    for (vector, byte) in vectors.iter().zip([0_u8, 17, 255]) {
        let parsed: HsId = vector["address"].as_str().unwrap().parse().unwrap();
        assert_eq!(parsed, HsId::from([byte; 32]));
    }
    println!("Arti synthetic address vectors: 3 passed");
}

#[cfg(not(feature = "tor"))]
fn main() {}
