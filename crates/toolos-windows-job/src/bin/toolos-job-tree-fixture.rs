#![cfg_attr(not(windows), allow(dead_code))]

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let mode = args.next().ok_or("missing fixture mode")?;
    let ready = args.next().ok_or("missing ready path")?;
    let survivor = args.next().ok_or("missing survivor path")?;

    match mode.to_string_lossy().as_ref() {
        "parent" => {
            let executable = env::current_exe()?;
            let child = Command::new(executable)
                .arg("child")
                .arg(&ready)
                .arg(&survivor)
                .spawn()?;
            println!("nested_child_pid={}", child.id());
            thread::sleep(Duration::from_secs(30));
        }
        "child" => {
            fs::write(Path::new(&ready), b"ready")?;
            thread::sleep(Duration::from_millis(1500));
            fs::write(Path::new(&survivor), b"survived")?;
            thread::sleep(Duration::from_secs(30));
        }
        other => return Err(format!("unknown fixture mode: {other}").into()),
    }

    Ok(())
}
