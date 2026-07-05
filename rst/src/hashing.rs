use anyhow::Result;
use sha2::{Digest, Sha512};
use std::io::Read;
use std::path::Path;

const CHUNK_SIZE: usize = 8192;

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn hash_bytes(data: &[u8]) -> String {
    hex::encode(Sha512::digest(data))
}

/// CurseForge Murmur2 fingerprint: strip whitespace bytes then hash.
pub fn murmur2(data: &[u8]) -> u32 {
    let filtered: Vec<u8> = data
        .iter()
        .copied()
        .filter(|&b| b != 9 && b != 10 && b != 13 && b != 32)
        .collect();
    murmur2_raw(&filtered, 1)
}

fn murmur2_raw(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1e995;
    const R: u32 = 24;

    let len = data.len() as u32;
    let mut h: u32 = seed ^ len;
    let mut i = 0;

    while i + 4 <= data.len() {
        let mut k = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
        i += 4;
    }

    let remaining = data.len() - i;
    if remaining >= 3 {
        h ^= (data[i + 2] as u32) << 16;
    }
    if remaining >= 2 {
        h ^= (data[i + 1] as u32) << 8;
    }
    if remaining >= 1 {
        h ^= data[i] as u32;
        h = h.wrapping_mul(M);
    }

    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

pub fn murmur2_file(path: &Path) -> Result<u32> {
    let data = std::fs::read(path)?;
    Ok(murmur2(&data))
}
