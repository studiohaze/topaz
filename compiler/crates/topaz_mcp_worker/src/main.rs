use std::io::{self, Write};

use topaz_mcp_worker::protocol::{WorkerRequest, WorkerResponse};

fn main() {
    let response = match WorkerRequest::read_from(&mut io::stdin().lock()) {
        Ok(request) => topaz_mcp_worker::execute(request),
        Err(error) => WorkerResponse::protocol_rejected(error.to_string()),
    };
    let mut frame = Vec::new();
    if response.write_to(&mut frame).is_err() {
        frame.clear();
        if WorkerResponse::host_limit("response")
            .write_to(&mut frame)
            .is_err()
        {
            std::process::exit(70);
        }
    }
    if io::stdout().lock().write_all(&frame).is_err() {
        std::process::exit(70);
    }
}
