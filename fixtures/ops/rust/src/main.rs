use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let mode = std::env::var("OPS_MODE").unwrap_or_default();
    println!(
        "stdout marker {}",
        std::env::var("OPS_SECRET").unwrap_or_default()
    );
    eprintln!(
        "stderr marker {}",
        std::env::var("OPS_SECRET").unwrap_or_default()
    );
    println!(
        "ambient {}",
        if std::env::var_os("UNGRANTED_SECRET").is_some() {
            "leaked"
        } else {
            "absent"
        }
    );
    if mode == "exit0" || mode == "exit7" {
        std::process::exit(if mode == "exit0" { 0 } else { 7 });
    }
    std::fs::write(
        std::env::var("OPS_PID_FILE").unwrap(),
        format!("{{\"parent\":{},\"children\":[]}}", std::process::id()),
    )?;
    if mode == "silent" {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let listener = TcpListener::bind((
        "127.0.0.1",
        std::env::var("OPS_PORT").unwrap().parse::<u16>().unwrap(),
    ))?;
    for connection in listener.incoming() {
        let mut connection = connection?;
        connection.set_read_timeout(Some(Duration::from_millis(250)))?;
        let mut request = String::new();
        if BufReader::new((&mut connection).take(4096))
            .read_line(&mut request)
            .is_err()
        {
            continue;
        }
        let status = if !request.starts_with("GET /health/ready ") {
            404
        } else if mode == "not_ready" {
            503
        } else if mode == "redirect" {
            302
        } else {
            200
        };
        let _ = write!(
            connection,
            "HTTP/1.1 {status} fixture\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 8\r\nConnection: close\r\n\r\nfixture\n"
        );
    }
    Ok(())
}
