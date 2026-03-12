use clap::Parser;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

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

/// Information about a connection handled by the server.
struct HandlingResult {
    /// Whether any message in this connection requested persistent server mode.
    stay_alive: bool,
    /// Whether any operation (move/copy) was successful.
    success: bool,
}

fn handle_connection(
    stream: UnixStream,
    destination_path: Arc<PathBuf>,
    quiet: bool,
    force_copy: bool,
) -> io::Result<HandlingResult> {
    let reader = BufReader::new(stream);
    let mut stay_alive = false;
    let mut success = false;

    for line in reader.lines() {
        match line {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }
                
                let mut parts = line.splitn(3, '|');
                let flags = parts.next().unwrap_or_default();
                let action = parts.next().unwrap_or_default();
                let source_path = parts.next().unwrap_or_default();

                if flags.contains('S') {
                    stay_alive = true;
                }

                let final_action = if force_copy || flags.contains('C') {
                    "copy"
                } else {
                    action
                };

                let source = Path::new(source_path);
                let destination = destination_path.join(
                    source
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("unknown")),
                );

                let (command, args) = match final_action {
                    "copy" => ("cp", vec!["-r", source_path]),
                    "move" => ("mv", vec![source_path]),
                    _ => {
                        if !action.is_empty() {
                            eprintln!("Invalid action: {}", action);
                        }
                        continue;
                    }
                };

                let output = Command::new(command).args(args).arg(&destination).output();

                match output {
                    Ok(output) => {
                        if !output.status.success() {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            eprintln!(
                                "Failed to {} {}: {}",
                                command,
                                source_path,
                                stderr
                            );
                        } else {
                            success = true;
                            if !quiet {
                                println!(
                                    "{} '{}' -> '{}'",
                                    if final_action == "copy" { "Copied" } else { "Moved" },
                                    source_path,
                                    destination.display()
                                );
                                let _ = io::stdout().flush();
                            }
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

    Ok(HandlingResult { stay_alive, success })
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

    let _ = std::fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH)?;
    
    let mut persistent = args.server;
    
    let op_mode = if args.copy { "copy mode" } else { "move mode" };
    
    if !args.quiet {
        if persistent {
            println!("Waiting (continuous mode, {})...", op_mode);
        } else {
            println!("Waiting (one-time mode, {})...", op_mode);
        }
        let _ = io::stdout().flush();
    }

    let destination_path = Arc::new(destination_path);

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let dest = Arc::clone(&destination_path);
                let q = args.quiet;
                let c = args.copy;
                
                match handle_connection(stream, dest, q, c) {
                    Ok(result) => {
                        if result.stay_alive && !persistent {
                            persistent = true;
                            if !args.quiet {
                                println!("Switched to continuous mode.");
                                println!("Waiting (continuous mode, {})...", op_mode);
                                let _ = io::stdout().flush();
                            }
                        }
                        
                        // Exit one-time mode only if at least one operation succeeded
                        if !persistent && result.success {
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error handling connection: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                break;
            }
        }
    }

    let _ = std::fs::remove_file(SOCKET_PATH);
    Ok(())
}

fn run_client(args: &Args) -> io::Result<()> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    
    for filename in &args.files {
        match fs::canonicalize(filename) {
            Ok(absolute_path) => {
                let action = if args.copy { "copy" } else { "move" };
                let mut flags = String::new();
                if args.server { flags.push('S'); }
                if args.copy { flags.push('C'); }

                let message = format!(
                    "{}|{}|{}\n",
                    flags,
                    action,
                    absolute_path.to_string_lossy()
                );

                stream.write_all(message.as_bytes())?;
                if !args.quiet {
                    println!("Sent: {}", absolute_path.display());
                    let _ = io::stdout().flush();
                }
            }
            Err(e) => {
                eprintln!("Error: Could not resolve '{}': {}", filename, e);
            }
        }
    }
    
    let _ = stream.flush();
    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    if args.files.is_empty() {
        if UnixStream::connect(SOCKET_PATH).is_ok() {
            eprintln!("Error: A toss server is already running. Provide files to toss, or stop the existing server.");
            std::process::exit(1);
        }
        run_server(&args)
    } else {
        match run_client(&args) {
            Ok(_) => Ok(()),
            Err(_) => {
                eprintln!("No toss server is running. Starting server in the current directory...");
                eprintln!("(Files argument ignored. Run toss with files from another terminal after the server starts.)");
                run_server(&args)
            }
        }
    }
}
