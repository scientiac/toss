use clap::Parser;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;

#[derive(Parser)]
#[command(name = "toss")]
#[command(author, version, about = "A throw-catch style move and copy program.", long_about = "\
A throw-catch style move and copy program.\n\n\
toss operates in two modes, detected automatically:\n\n  \
Server Mode: Run 'toss' without files to start receiving.\n             \
Files sent from other terminals will be placed in the current directory.\n\n  \
Client Mode: Run 'toss <files>' while a server is running to send files.\n             \
Files are moved by default, use -c to copy instead.\n\n\
All flags (-c, -s, -q, -d) can be passed in either mode.")]
struct Args {
    /// Files to toss
    files: Vec<String>,

    /// Copy files instead of moving
    #[arg(short, long)]
    copy: bool,

    /// Keep running for multiple transfers
    #[arg(short, long)]
    server: bool,

    /// Execute tasks quietly
    #[arg(short, long)]
    quiet: bool,

    /// Set the destination directory
    #[arg(short, long)]
    destination: Option<String>,
}

const SOCKET_PATH: &str = "/tmp/yeetyeetyeet";

fn handle_connection(
    stream: UnixStream,
    destination_path: Arc<PathBuf>,
    quiet: bool,
    force_copy: bool,
) -> io::Result<()> {
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        match line {
            Ok(line) => {
                let mut parts = line.splitn(2, '|');
                let action = parts.next().unwrap_or_default();
                let source_path = parts.next().unwrap_or_default();

                // Server's -c flag overrides: force everything to copy
                let action = if force_copy { "copy" } else { action };

                let source = Path::new(source_path);
                let destination = destination_path.join(
                    source
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("unknown")),
                );

                let (command, args) = match action {
                    "copy" => ("cp", vec!["-r", source_path]),
                    "move" => ("mv", vec![source_path]),
                    _ => {
                        eprintln!("Invalid action: {}", action);
                        continue;
                    }
                };

                let output = Command::new(command).args(args).arg(&destination).output();

                match output {
                    Ok(output) => {
                        if !output.status.success() {
                            eprintln!(
                                "Failed to {} {}: {}",
                                command,
                                source_path,
                                String::from_utf8_lossy(&output.stderr)
                            );
                        } else if !quiet {
                            println!(
                                "{} '{}' -> '{}'",
                                if command == "cp" { "Copied" } else { "Moved" },
                                source_path,
                                destination.display()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("Error executing {} command: {}", command, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading line: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn run_server(args: &Args) -> io::Result<()> {
    let destination_path: PathBuf = if let Some(dest) = &args.destination {
        let path = Path::new(dest);
        if !path.is_dir() {
            eprintln!("Error: '{}' is not a valid directory.", dest);
            std::process::exit(1);
        }
        path.to_path_buf()
    } else {
        std::env::current_dir()?
    };

    // Clean up any stale socket file from a previous run
    if Path::new(SOCKET_PATH).exists() {
        std::fs::remove_file(SOCKET_PATH)?;
    }

    let listener = UnixListener::bind(SOCKET_PATH)?;
    if !args.quiet {
        println!("Waiting...");
    }

    let destination_path = Arc::new(destination_path);

    let result = if args.server {
        run_server_loop(&listener, &destination_path, args.quiet, args.copy)
    } else {
        run_server_once(&listener, &destination_path, args.quiet, args.copy)
    };

    // Clean up socket on exit
    let _ = std::fs::remove_file(SOCKET_PATH);

    result
}

fn run_server_loop(
    listener: &UnixListener,
    destination_path: &Arc<PathBuf>,
    quiet: bool,
    force_copy: bool,
) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let destination_path = Arc::clone(destination_path);
                thread::spawn(move || {
                    if let Err(e) =
                        handle_connection(stream, destination_path, quiet, force_copy)
                    {
                        eprintln!("Error handling connection: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
            }
        }
    }
    Ok(())
}

fn run_server_once(
    listener: &UnixListener,
    destination_path: &Arc<PathBuf>,
    quiet: bool,
    force_copy: bool,
) -> io::Result<()> {
    match listener.accept() {
        Ok((stream, _)) => {
            let destination_path = Arc::clone(destination_path);
            let handle = thread::spawn(move || {
                handle_connection(stream, destination_path, quiet, force_copy).unwrap_or_else(
                    |e| {
                        eprintln!("Error handling connection: {}", e);
                    },
                );
            });

            handle.join().expect("Thread panicked");
        }
        Err(e) => {
            eprintln!("Failed to accept connection: {}", e);
        }
    }
    Ok(())
}

fn run_client(args: &Args) -> io::Result<()> {
    match UnixStream::connect(SOCKET_PATH) {
        Ok(mut stream) => {
            for filename in &args.files {
                match fs::canonicalize(filename) {
                    Ok(absolute_path) => {
                        let action = if args.copy { "copy" } else { "move" };
                        let message = format!("{}|{}\n", action, absolute_path.to_string_lossy());

                        stream.write_all(message.as_bytes())?;
                        if !args.quiet {
                            println!("Sent: {} ({})", absolute_path.display(), action);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: Could not resolve '{}': {}", filename, e);
                    }
                }
            }
        }
        Err(_) => {
            eprintln!(
                "Error: Could not connect to the server. Make sure toss is running as a server first."
            );
        }
    }

    Ok(())
}

/// Check if a server is actually listening on the socket (not just a stale file).
fn server_is_running() -> bool {
    UnixStream::connect(SOCKET_PATH).is_ok()
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    if args.files.is_empty() {
        if server_is_running() {
            eprintln!("Error: A toss server is already running. Provide files to toss, or stop the existing server.");
            std::process::exit(1);
        }
        // No live server, no files → start server
        run_server(&args)?;
    } else if server_is_running() {
        // Live server and files provided → send files
        run_client(&args)?;
    } else {
        // No live server but files provided → start server, ignoring files
        eprintln!("No toss server is running. Starting server in the current directory...");
        eprintln!("(Files argument ignored. Run toss with files from another terminal after the server starts.)");
        run_server(&args)?;
    }

    Ok(())
}
