use super::docker;

#[derive(Serialize, Deserialize, Debug)]
pub struct InvokeRequestBody {
    pub stdin: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct InvokeResponseBody {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
    pub duration: u64,
    pub error: Option<String>,
}

use crate::{Result, ResultExt};

pub fn run(container_id: &str, port: u16, program: &str, stdin: &[u8]) -> Result<(String, String, i32, u64)> {
    let container_id = match docker::get_container_id(container_id)? {
        Some(container_id) => container_id,
        None => bail!("Unable to find a running container with id {}.", container_id),
    };

    let addr = match docker::get_public_addr(container_id.as_str(), port)? {
        Some(addr) => addr,
        None => bail!("Unable to find public address of cds server running in container {}. Is the port exposed?", container_id),
    };

    let req_body = InvokeRequestBody {
        stdin: base64::encode(stdin),
    };

    let response: InvokeResponseBody = reqwest::ClientBuilder::new()
        .timeout(None)
        .build()?

        .post(format!("http://{}/run/{}", addr, program).as_str())
        .json(&req_body)
        .send()
        .chain_err(|| "Server issue")?

        .json()
        .chain_err(|| "Server response could not be parsed")?;
    
    if response.error.is_some() {
        return Err(response.error.unwrap().into())
    }

    Ok((
        String::from_utf8_lossy(base64::decode(&response.stdout).chain_err(|| "Could not decode stdout")?.as_slice()).into_owned(),
        String::from_utf8_lossy(base64::decode(&response.stderr).chain_err(|| "Could not decode stderr")?.as_slice()).into_owned(),
        response.exit_status,
        response.duration
    ))
}
