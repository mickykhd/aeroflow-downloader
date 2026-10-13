use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncReadExt;

/// Stream a file and return lowercase hex SHA-256.
pub async fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn verify(expected_hex: &str, actual_hex: &str) -> bool {
    expected_hex.trim().eq_ignore_ascii_case(actual_hex.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hello.bin");
        tokio::fs::write(&p, b"hello").await.unwrap();
        let h = sha256_file(&p).await.unwrap();
        assert_eq!(h, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert!(verify("2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824", &h));
    }
}
