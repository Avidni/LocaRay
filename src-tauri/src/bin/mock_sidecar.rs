use std::{env, io::Write, process::ExitCode, thread, time::Duration};

fn main() -> ExitCode {
    let Some(mode) = env::args().nth(1) else {
        eprintln!("A mock scenario is required.");
        return ExitCode::from(2);
    };

    match mode.as_str() {
        "success-stdout" => {
            println!("Tunnel ready at https://mock-success.trycloudflare.com");
            ExitCode::SUCCESS
        }
        "success-stderr" => {
            eprintln!("Tunnel ready at https://mock-stderr.trycloudflare.com");
            ExitCode::SUCCESS
        }
        "split-stderr" => {
            let mut stderr = std::io::stderr().lock();
            if stderr.write_all(b"Tunnel ready at https://mock-").is_err()
                || stderr.flush().is_err()
            {
                return ExitCode::from(3);
            }
            thread::sleep(Duration::from_millis(20));
            if stderr
                .write_all(b"split.trycloudflare.com\n")
                .and_then(|()| stderr.flush())
                .is_err()
            {
                return ExitCode::from(3);
            }
            ExitCode::SUCCESS
        }
        "invalid-url" => {
            eprintln!("Rejected candidate https://trycloudflare.com.attacker.example");
            ExitCode::SUCCESS
        }
        "exit-early" => ExitCode::from(23),
        _ => {
            eprintln!("Unknown mock scenario.");
            ExitCode::from(2)
        }
    }
}
