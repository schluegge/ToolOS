use std::{env, fs, path::PathBuf};

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut quartet = [0_u8; 4];
    let mut quartet_len = 0_usize;

    for byte in input.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => 64,
            _ => return Err(format!("invalid base64 byte: {byte}")),
        };

        quartet[quartet_len] = value;
        quartet_len += 1;

        if quartet_len == 4 {
            if quartet[0] == 64 || quartet[1] == 64 {
                return Err("invalid base64 padding".to_owned());
            }

            output.push((quartet[0] << 2) | (quartet[1] >> 4));
            if quartet[2] != 64 {
                output.push((quartet[1] << 4) | (quartet[2] >> 2));
            }
            if quartet[3] != 64 {
                if quartet[2] == 64 {
                    return Err("invalid base64 padding order".to_owned());
                }
                output.push((quartet[2] << 6) | quartet[3]);
            }

            quartet_len = 0;
        }
    }

    if quartet_len != 0 {
        return Err("incomplete base64 quartet".to_owned());
    }

    Ok(output)
}

fn ensure_windows_icon() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icon_dir = manifest_dir.join("icons");
    let encoded_path = icon_dir.join("icon.ico.base64");
    let icon_path = icon_dir.join("icon.ico");

    println!("cargo:rerun-if-changed={}", encoded_path.display());

    if icon_path.exists() {
        return;
    }

    let encoded = fs::read_to_string(&encoded_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", encoded_path.display()));
    let icon = decode_base64(&encoded)
        .unwrap_or_else(|error| panic!("failed to decode {}: {error}", encoded_path.display()));

    fs::create_dir_all(&icon_dir)
        .unwrap_or_else(|error| panic!("failed to create {}: {error}", icon_dir.display()));
    fs::write(&icon_path, icon)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", icon_path.display()));
}

fn main() {
    ensure_windows_icon();
    tauri_build::build();
}
