// Mirror of Jackett.Server/Program.cs — the console entry point.
//
// Keeping the CLI here (rather than in the library) lets run.bat / run.sh
// forward options directly to the binary, while jackett-service calls
// `run_server` in-process with the same `RuntimeServerConfig`.
use std::path::PathBuf;

use jackett_server::{run_server, RuntimeServerConfig, RuntimeSettings};

const USAGE: &str = concat!(
    "Usage: jackett-server [options]\n",
    "  -p, --port <n>              HTTP port (overrides ServerConfig)\n",
    "  -d, --data-folder <path>    configuration/data directory\n",
    "      --definitions-dir <dir> Cardigann YAML definitions\n",
    "  -x, --listen-public         bind to 0.0.0.0\n",
    "  -z, --listen-private        bind to 127.0.0.1\n",
    "      --pid-file <path>       write the process id on startup\n",
    "      --tracing               enable verbose request tracing\n",
    "      --no-updates            disable the update checker\n",
    "      --no-restart            do not restart after a crash\n",
    "  -h, --help                  show this text",
);

fn value<I: Iterator<Item = String>>(it: &mut I, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_args() -> Result<(RuntimeSettings, RuntimeServerConfig), String> {
    let mut runtime = RuntimeSettings::default();
    let mut server = RuntimeServerConfig::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-p" | "--port" => {
                let raw = value(&mut it, &arg)?;
                server.port_override = Some(
                    raw.parse::<u16>()
                        .map_err(|_| format!("invalid port: {raw}"))?,
                );
            }
            "-d" | "--data-folder" => runtime.custom_data_folder = Some(value(&mut it, &arg)?),
            "--definitions-dir" => {
                server.definitions_dir = Some(PathBuf::from(value(&mut it, &arg)?))
            }
            "-x" | "--listen-public" => {
                server.bind_address_override = Some("0.0.0.0".into())
            }
            "-z" | "--listen-private" => {
                server.bind_address_override = Some("127.0.0.1".into())
            }
            "--pid-file" => runtime.pid_file = Some(value(&mut it, &arg)?),
            "--tracing" => runtime.tracing_enabled = true,
            "--no-updates" => runtime.no_updates = true,
            "--no-restart" => runtime.no_restart = true,
            other => return Err(format!("unknown argument: {other}\n{USAGE}")),
        }
    }
    Ok((runtime, server))
}

#[tokio::main]
async fn main() {
    if std::env::args().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }

    let (runtime, server) = match parse_args() {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    if let Some(path) = &runtime.pid_file {
        if let Err(e) = std::fs::write(path, std::process::id().to_string()) {
            eprintln!("failed to write pid file {path}: {e}");
        }
    }

    let result = run_server(runtime.clone(), server).await;

    if let Some(path) = &runtime.pid_file {
        let _ = std::fs::remove_file(path);
    }
    if let Err(e) = result {
        eprintln!("jackett-server failed: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn usage_documents_batch_options() {
        assert!(super::USAGE.contains("--port"));
        assert!(super::USAGE.contains("--data-folder"));
        assert!(super::USAGE.contains("--definitions-dir"));
    }
}
