use std::process::{Command};
use std::str;
use std::net::SocketAddr;

use std::str::FromStr;

use super::{Result, ResultExt};

// TODO: fail when container or image search is ambiguous
fn run(c: &mut Command) -> Result<String> {
    let output = c.output()
        .chain_err(|| "Unable to invoke docker client")?;

    if !output.status.success() {
        if let Ok(stderr) = String::from_utf8(output.stderr) {
            bail!("Invocation of docker client terminated unsuccessfully with exit code {}: {}",
                output.status.code().unwrap_or(-1), stderr);
        } else {
            bail!("Invocation of docker client terminated unsuccessfully with exit code {}\n: stderr of docker client is not decodable (contains non-utf8 signs)",
                output.status.code().unwrap_or(-1));
        }
    }

    Ok(
        String::from_utf8_lossy(
            output
                .stdout
                .as_slice()
        )
            .to_string()
    )
}

pub fn check() -> Result<()> {
    run(
        Command::new("docker").arg("version")
    )
        .chain_err(|| "Invoking docker client failed!")?;

    //let stdout = String::from(output.stdout);
    // TODO: parse output

    Ok(())
}

pub fn get_container_id(id_name: &str) -> Result<Option<String>> {
    let output = run(Command::new("docker")
        .arg("ps")
        .arg("--format")
        .arg("{{.ID}}:{{.Names}}"))?;

    for entry in output.split("\n") {
        let line: Vec<&str> = entry.split(":").collect();
        if line.len() != 2 {
            continue
        };

        let (id, name) = (line[0], line[1]);

        if id.starts_with(id_name) || name.starts_with(id_name) {
            return Ok(Some(id.to_owned()));
        }
    }

    Ok(None)
}

pub fn get_image_id(id_name: &str) -> Result<Option<String>> {
    let output = run(Command::new("docker")
        .arg("images")
        .arg("--format")
        .arg("{{.ID}} {{.Repository}}:{{.Tag}}"))?;

    for entry in output.split("\n") {
        let line: Vec<&str> = entry.split(" ").collect();
        if line.len() != 2 {
            continue
        };

        let (id, name) = (line[0], line[1]);

        if id.starts_with(id_name) || name.starts_with(id_name) {
            return Ok(Some(id.to_owned()));
        }
    }

    Ok(None)
}

pub fn get_public_addr(container_id: &str, port: u16) -> Result<Option<SocketAddr>> {
    let output = run(Command::new("docker")
        .arg("port")
        .arg(container_id))?;

    for entry in output.split("\n") {
        let line: Vec<&str> = entry.split(" -> ").collect();
        if line.len() != 2 {
            continue
        };

        let (inner, outer) = (line[0], line[1]);

        if inner == format!("{}/tcp", port) {
            let addr = SocketAddr::from_str(outer)
                .chain_err(|| format!("unable to parse container's {} public address ({}) associate with private port {}", container_id, outer, port))?;
            return Ok(Some(addr));
        }
    }

    Ok(None)
}

pub fn start_container(image_id: &str, options: &[&str]) -> Result<String> {
    let mut cmd = Command::new("docker");
    cmd.arg("run")
       .arg("-d");

    for option in options.iter() {
        cmd.arg(option);
    }

    Ok(
        run(cmd.arg(image_id))?
            .trim()[..12]
            .to_owned()
    )
}

pub fn stop_container(container_id: &str, remove: bool) -> Result<()> {
    let mut cmd = Command::new("docker");

    if remove {
        cmd.arg("rm")
           .arg("--force");
    } else {
        cmd.arg("stop");
    }

    run(cmd.arg(container_id))?;

    Ok(())
}
