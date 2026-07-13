#[cfg(unix)]
use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    thread,
    time::Duration,
};

#[cfg(unix)]
fn main() {
    let arguments = env::args().collect::<Vec<_>>();
    match arguments.get(1).map(String::as_str) {
        Some("serve") => serve(Path::new(&arguments[2])),
        Some("drive") => drive(Path::new(&arguments[2])),
        _ => panic!("expected serve or drive with a Unix socket path"),
    }
}

#[cfg(not(unix))]
fn main() {}

#[cfg(unix)]
struct ScenarioRequest<'a> {
    kind: &'a str,
    delay_ms: u64,
    deadline_ms: u64,
    capacity: u32,
    demand: u32,
    idempotent: bool,
    max_attempts: u32,
}

#[cfg(unix)]
impl<'a> ScenarioRequest<'a> {
    fn parse(value: &'a str) -> Self {
        let fields = value.trim().split('|').collect::<Vec<_>>();
        Self {
            kind: fields[0],
            delay_ms: fields[1].parse().unwrap(),
            deadline_ms: fields[2].parse().unwrap(),
            capacity: fields[3].parse().unwrap(),
            demand: fields[4].parse().unwrap(),
            idempotent: fields[5].parse().unwrap(),
            max_attempts: fields[6].parse().unwrap(),
        }
    }
}

#[cfg(unix)]
struct ScenarioObservation<'a> {
    outcome: &'a str,
    attempts: u32,
    retried: bool,
    retry_reason: &'a str,
    controlled_time_end_ms: u64,
    health_reason: &'a str,
}

#[cfg(unix)]
impl ScenarioObservation<'_> {
    fn to_json(&self) -> String {
        format!(
            "{{\"artifactVersion\":\"lenso.sandbox-workload-observation.v1\",\"outcome\":\"{}\",\"attempts\":{},\"retryAttempted\":{},\"retryReason\":\"{}\",\"controlledTimeEndMs\":{},\"finalHealth\":\"ready\",\"healthReason\":\"{}\"}}",
            self.outcome,
            self.attempts,
            self.retried,
            self.retry_reason,
            self.controlled_time_end_ms,
            self.health_reason
        )
    }
}

#[cfg(unix)]
fn serve(socket: &Path) {
    let _ = fs::remove_file(socket);
    let listener = UnixListener::bind(socket).unwrap();
    for connection in listener.incoming() {
        let mut connection = connection.unwrap();
        let mut request = String::new();
        BufReader::new(connection.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        let request = ScenarioRequest::parse(&request);

        let observation = match request.kind {
            "timeout" | "slow_dependency" => {
                assert!(request.delay_ms >= request.deadline_ms);
                let health_reason = if request.kind == "timeout" {
                    "workload_deadline_rejected"
                } else {
                    "dependency_deadline_rejected"
                };
                ScenarioObservation {
                    outcome: "deadline_exceeded",
                    attempts: 1,
                    retried: false,
                    retry_reason: "deadline_exhausted",
                    controlled_time_end_ms: request.delay_ms,
                    health_reason,
                }
            }
            "overload" => {
                let admitted = request.demand.min(request.capacity);
                let rejected = request.demand - admitted;
                assert!(rejected > 0);
                let attempts = if request.idempotent {
                    request.max_attempts
                } else {
                    1
                };
                ScenarioObservation {
                    outcome: "overload_rejected",
                    attempts,
                    retried: attempts > 1,
                    retry_reason: if attempts > 1 {
                        "retry_limit_reached"
                    } else {
                        "unsafe_operation"
                    },
                    controlled_time_end_ms: 0,
                    health_reason: "workload_capacity_gate_rejected",
                }
            }
            _ => panic!("unsupported injected fault"),
        };
        writeln!(connection, "{}", observation.to_json()).unwrap();
    }
}

#[cfg(unix)]
fn drive(socket: &Path) {
    let mut connection = (0..1_000)
        .find_map(|_| match UnixStream::connect(socket) {
            Ok(connection) => Some(connection),
            Err(_) => {
                thread::sleep(Duration::from_millis(1));
                None
            }
        })
        .expect("managed Workload socket did not become ready");
    let value = |name: &str| env::var(name).unwrap();
    writeln!(
        connection,
        "{}|{}|{}|{}|{}|{}|{}",
        value("LENSO_SANDBOX_FAULT_KIND"),
        value("LENSO_SANDBOX_DELAY_MS"),
        value("LENSO_SANDBOX_DEADLINE_MS"),
        value("LENSO_SANDBOX_CAPACITY"),
        value("LENSO_SANDBOX_DEMAND"),
        value("LENSO_SANDBOX_IDEMPOTENT"),
        value("LENSO_SANDBOX_MAX_ATTEMPTS")
    )
    .unwrap();
    let mut observation = String::new();
    BufReader::new(connection)
        .read_line(&mut observation)
        .unwrap();
    print!("{observation}");
}
