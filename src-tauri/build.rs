use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    tauri_build::build();
    println!("cargo:rerun-if-env-changed=FERRA_SEAL_NAME");
    println!("cargo:rerun-if-env-changed=FERRA_SEAL_SALT");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let stamp = out.join("ferra-seal.txt");
    let (name, salt) = match (
        std::env::var("FERRA_SEAL_NAME").ok().filter(|s| !s.is_empty()),
        std::env::var("FERRA_SEAL_SALT").ok().filter(|s| !s.is_empty()),
    ) {
        (Some(name), Some(salt)) => (name, salt),
        _ => {
            if let Ok(raw) = fs::read_to_string(&stamp) {
                let mut lines = raw.lines();
                if let (Some(n), Some(s)) = (lines.next(), lines.next()) {
                    if !n.is_empty() && !s.is_empty() {
                        (n.to_string(), s.to_string())
                    } else {
                        fresh_pair()
                    }
                } else {
                    fresh_pair()
                }
            } else {
                fresh_pair()
            }
        }
    };
    let _ = fs::write(&stamp, format!("{name}\n{salt}\n"));
    println!("cargo:rustc-env=FERRA_SEAL_NAME={name}");
    println!("cargo:rustc-env=FERRA_SEAL_SALT={salt}");
}

fn fresh_pair() -> (String, String) {
    (random_hex(16), random_hex(32))
}

fn random_hex(nbytes: usize) -> String {
    let mut x = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0x9e3779b97f4a7c15)
        ^ ((std::process::id() as u128) << 32)
        ^ (std::ptr::addr_of!(nbytes) as u128);
    let mut out = String::with_capacity(nbytes * 2);
    for _ in 0..nbytes {
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 8;
        x = x.wrapping_mul(0x2545F4914F6CDD1D);
        out.push_str(&format!("{:02x}", x as u8));
    }
    out
}
