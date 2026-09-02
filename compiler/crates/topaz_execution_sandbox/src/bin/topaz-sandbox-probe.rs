use std::fs;
use std::io;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::process::Command;

use topaz_execution_sandbox::protocol::{ProbeRequest, ProbeResponse};

fn main() {
    let request = match ProbeRequest::read_from(&mut io::stdin().lock()) {
        Ok(request) => request,
        Err(_) => std::process::exit(64),
    };
    let response = ProbeResponse {
        inherited_environment_entries: std::env::vars().count() as u32,
        filesystem_read_denied: denied(fs::read(&request.forbidden_read_path)),
        filesystem_write_denied: denied(fs::write(&request.forbidden_write_path, b"probe")),
        network_denied: denied(TcpStream::connect("127.0.0.1:9"))
            && denied(TcpListener::bind("127.0.0.1:0"))
            && denied(UdpSocket::bind("127.0.0.1:0")),
        child_process_denied: denied(Command::new("/usr/bin/true").status()),
    };
    if response.write_to(&mut io::stdout().lock()).is_err() {
        std::process::exit(70);
    }
}

fn denied<T>(result: io::Result<T>) -> bool {
    result
        .err()
        .is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied)
}
