use std::io;

use topaz_execution_sandbox::protocol::{WorkerRequest, WorkerResponse};

fn main() {
    let response = match WorkerRequest::read_from(&mut io::stdin().lock()) {
        Ok(request) => topaz_execution_sandbox::worker::execute(request),
        Err(error) => WorkerResponse::protocol_rejected(error.to_string()),
    };
    if response.write_to(&mut io::stdout().lock()).is_err() {
        std::process::exit(70);
    }
}
