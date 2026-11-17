use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

// Paleta de colores Cyberpunk
const RED: &str = "\x1b[38;2;255;0;60m";      // Neon Red
const YELLOW: &str = "\x1b[38;2;254;211;48m"; // Cyber Yellow
const CYAN: &str = "\x1b[38;2;0;240;255m";    // Net Cyan
const GREEN: &str = "\x1b[38;2;46;213;115m";  // Matrix Green
const GRAY: &str = "\x1b[38;2;120;120;120m";  // Dark Chrome
const WHITE: &str = "\x1b[38;2;255;255;255m"; // Pure White
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[cfg(windows)]
fn enable_ansi_support() {
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetStdHandle(n: u32) -> *mut std::ffi::c_void;
            fn GetConsoleMode(h: *mut std::ffi::c_void, m: *mut u32) -> i32;
            fn SetConsoleMode(h: *mut std::ffi::c_void, m: u32) -> i32;
        }
        let h = GetStdHandle(0xFFFFFFF5);
        if !h.is_null() && h as isize != -1 {
            let mut m: u32 = 0;
            if GetConsoleMode(h, &mut m) != 0 {
                let _ = SetConsoleMode(h, m | 0x0004);
            }
        }
    }
}

#[derive(Debug)]
struct ProjectTarget {
    name: String,
    path: PathBuf,
    source_tag: String,
}

struct SyncStats {
    files_copied: usize,
    files_skipped: usize,
    bytes_transferred: u64,
}

fn get_usb_root() -> PathBuf {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(std::path::Component::Prefix(prefix)) = exe_path.components().next() {
            let s = prefix.as_os_str().to_string_lossy();
            let clean = s.strip_prefix(r"\\?\").unwrap_or(&s);
            return PathBuf::from(format!("{}\\", clean.trim_end_matches('\\')));
        }
    }
    PathBuf::from(".\\")
}

fn scan_projects(user_profile: &Path) -> Vec<ProjectTarget> {
    let mut targets = Vec::new();
    let mut seen_paths = HashSet::new();

    let default_idea = user_profile.join("IdeaProjects");
    if default_idea.is_dir() {
        if let Ok(entries) = fs::read_dir(&default_idea) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                    if seen_paths.insert(canon) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        targets.push(ProjectTarget {
                            name,
                            path: p,
                            source_tag: "IdeaProjects".to_string(),
                        });
                    }
                }
            }
        }
    }

    if let Ok(appdata) = env::var("APPDATA") {
        let jb_path = Path::new(&appdata).join("JetBrains");
        if jb_path.is_dir() {
            if let Ok(dirs) = fs::read_dir(jb_path) {
                for d in dirs.flatten() {
                    let d_path = d.path();
                    let d_name = d.file_name().to_string_lossy().to_string();
                    if d_name.starts_with("IntelliJIdea") {
                        let recent_xml = d_path.join("options").join("recentProjects.xml");
                        if recent_xml.is_file() {
                            parse_recent_projects_xml(&recent_xml, user_profile, &mut targets, &mut seen_paths);
                        }
                    }
                }
            }
        }
    }

    let docs_idea = user_profile.join("Documents").join("IdeaProjects");
    if docs_idea.is_dir() && docs_idea != default_idea {
        if let Ok(entries) = fs::read_dir(&docs_idea) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                    if seen_paths.insert(canon) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        targets.push(ProjectTarget {
                            name,
                            path: p,
                            source_tag: "Documents".to_string(),
                        });
                    }
                }
            }
        }
    }

    targets
}

fn parse_recent_projects_xml(
    xml_path: &Path,
    user_profile: &Path,
    targets: &mut Vec<ProjectTarget>,
    seen_paths: &mut HashSet<PathBuf>,
) {
    if let Ok(file) = File::open(xml_path) {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if let Some(start) = line.find("<entry key=\"") {
                let rest = &line[start + 12..];
                if let Some(end) = rest.find('"') {
                    let raw_path = &rest[..end];
                    let resolved_str = raw_path
                        .replace("$USER_HOME$", &user_profile.to_string_lossy())
                        .replace('/', "\\");
                    let p = PathBuf::from(resolved_str);
                    if p.is_dir() {
                        let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                        if seen_paths.insert(canon) {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "project".to_string());
                            targets.push(ProjectTarget {
                                name,
                                path: p,
                                source_tag: "IDE History".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn sync_directory_recursive(src: &Path, dst: &Path, stats: &mut SyncStats) -> io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            sync_directory_recursive(&src_path, &dst_path, stats)?;
        } else if src_path.is_file() {
            let mut should_copy = true;
            if dst_path.is_file() {
                if let (Ok(src_meta), Ok(dst_meta)) = (src_path.metadata(), dst_path.metadata()) {
                    if src_meta.len() == dst_meta.len() {
                        if let (Ok(src_time), Ok(dst_time)) = (src_meta.modified(), dst_meta.modified()) {
                            if src_time <= dst_time {
                                should_copy = false;
                            }
                        }
                    }
                }
            }

            if should_copy {
                let bytes = fs::copy(&src_path, &dst_path)?;
                stats.files_copied += 1;
                stats.bytes_transferred += bytes;
            } else {
                stats.files_skipped += 1;
            }
        }
    }

    Ok(())
}

fn print_terminal_banner(host: &str, user: &str, usb: &Path) {
    println!("{RED}{BOLD}");
    println!(r#"  ███████╗██╗   ██╗██████╗       ███╗   ██╗███████╗████████╗"#);
    println!(r#"  ██╔════╝██║   ██║██╔══██╗      ████╗  ██║██╔════╝╚══██╔══╝"#);
    println!(r#"  ███████╗██║   ██║██████╔╝█████╗██╔██╗ ██║█████╗     ██║   "#);
    println!(r#"  ╚════██║██║   ██║██╔══██╗╚════╝██║╚██╗██║██╔══╝     ██║   "#);
    println!(r#"  ███████║╚██████╔╝██████╔╝      ██║ ╚████║███████╗   ██║   "#);
    println!(r#"  ╚══════╝ ╚═════╝ ╚═════╝       ╚═╝  ╚═══╝╚══════╝   ╚═╝   "#);
    println!("{RESET}{YELLOW}       NEURAL SUB-NET PROJECT REPLICATOR // v1.0.1{RESET}");
    println!("{GRAY}----------------------------------------------------------------------{RESET}");
    println!(" {CYAN}[SYSTEM IDENT]{RESET}  NODE_HOST: {WHITE}{host}{RESET} | AGENT_USER: {WHITE}{user}{RESET}");
    println!(" {CYAN}[DATA VAULT ]{RESET}  REMOVABLE VAULT: {GREEN}{}{RESET}", usb.display());
    println!(" {CYAN}[SECURITY   ]{RESET}  CLEARANCE: {RED}{BOLD}LEVEL 5 // SECURE SYNC PROTOCOL{RESET}");
    println!("{GRAY}----------------------------------------------------------------------{RESET}\n");
}

fn main() {
    #[cfg(windows)]
    enable_ansi_support();

    let host_name = env::var("COMPUTERNAME").unwrap_or_else(|_| "UNKNOWN_NODE".to_string());
    let user_name = env::var("USERNAME").unwrap_or_else(|_| "AGENT".to_string());
    let usb_root = get_usb_root();

    print_terminal_banner(&host_name, &user_name, &usb_root);

    let user_profile_str = env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
    let user_profile = Path::new(&user_profile_str);

    println!("{YELLOW}{BOLD}[+] DEPLOYING DISCOVERY PROBE ON LOCAL FILE CHANNELS...{RESET}");
    let mut targets = scan_projects(user_profile);

    if targets.is_empty() {
        println!("{RED}[-] PROBE ALERT: No IDE project clusters detected automatically.{RESET}");
        println!("{CYAN}[?] Enter custom project directory path (or press ENTER to abort):{RESET} ");
        let mut custom = String::new();
        let _ = io::stdin().read_line(&mut custom);
        let custom = custom.trim();
        if !custom.is_empty() {
            let cp = PathBuf::from(custom);
            if cp.is_dir() {
                if let Ok(entries) = fs::read_dir(&cp) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            targets.push(ProjectTarget {
                                name: entry.file_name().to_string_lossy().to_string(),
                                path: entry.path(),
                                source_tag: "Manual Path".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    if targets.is_empty() {
        println!("{GRAY}[!] PROBE TERMINATED. NO TARGET CLUSTERS FOUND.{RESET}");
        println!("\n{CYAN}Press ENTER to close terminal...{RESET}");
        let mut _exit = String::new();
        let _ = io::stdin().read_line(&mut _exit);
        return;
    }

    println!("{GREEN}[✓] SCAN COMPLETE: {BOLD}{}{RESET}{GREEN} CLUSTERS READY FOR SYNCHRONIZATION:{RESET}\n", targets.len());

    for (idx, target) in targets.iter().enumerate() {
        println!(
            "  {RED}[{:02}]{RESET} {WHITE}{:<28}{RESET} {GRAY}// SOURCE: {}{RESET}",
            idx + 1,
            target.name,
            target.source_tag
        );
    }

    let dest_base = usb_root.join("Respaldos_IntelliJ").join("Proyectos");
    let log_dir = usb_root.join("Respaldos_IntelliJ").join("recursos");
    let _ = fs::create_dir_all(&dest_base);
    let _ = fs::create_dir_all(&log_dir);

    println!("\n{GRAY}----------------------------------------------------------------------{RESET}");
    println!(" {YELLOW}[VAULT TARGET]{RESET} {}", dest_base.display());
    println!(" {YELLOW}[PROTOCOL    ]{RESET} Incremental Multi-Cluster Replication (Zero-Loss / Fast-Sync)");
    println!("{GRAY}----------------------------------------------------------------------{RESET}\n");

    print!("{YELLOW}{BOLD}[SUB-NET PROTOCOL] -> Press [ENTER] to replicate (or 'Q' to abort): {RESET}");
    let _ = io::stdout().flush();

    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    if input.trim().eq_ignore_ascii_case("q") {
        println!("\n{RED}[!] REPLICATION ABORTED BY USER.{RESET}");
        return;
    }

    println!("\n{CYAN}{BOLD}[+] SYNCHRONIZING CLUSTERS TO REMOVABLE VAULT...{RESET}\n");

    let timer = Instant::now();
    let mut total_copied = 0;
    let mut total_skipped = 0;
    let mut total_bytes = 0u64;

    let log_file_path = log_dir.join("subnet_audit.log");
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .ok();

    if let Some(ref mut f) = log_file {
        let _ = writeln!(f, "\n=== SUB-NET DATA AUDIT // HOST: {} // USER: {} ===", host_name, user_name);
    }

    for (idx, target) in targets.iter().enumerate() {
        print!(
            "  {GRAY}[{:02}/{}]{RESET} {WHITE}{:<28}{RESET} ... ",
            idx + 1,
            targets.len(),
            target.name
        );
        let _ = io::stdout().flush();

        let target_dest = dest_base.join(&target.name);
        let mut stats = SyncStats {
            files_copied: 0,
            files_skipped: 0,
            bytes_transferred: 0,
        };

        match sync_directory_recursive(&target.path, &target_dest, &mut stats) {
            Ok(_) => {
                total_copied += stats.files_copied;
                total_skipped += stats.files_skipped;
                total_bytes += stats.bytes_transferred;

                if stats.files_copied > 0 {
                    let mb = stats.bytes_transferred as f64 / (1024.0 * 1024.0);
                    println!(
                        "{GREEN}[EXTRACTED // {} FILES // {:.2} MB]{RESET}",
                        stats.files_copied, mb
                    );
                } else {
                    println!("{GRAY}[UP-TO-DATE // NO CHG]{RESET}");
                }

                if let Some(ref mut f) = log_file {
                    let _ = writeln!(
                        f,
                        "CLUSTER: {} | COPIED: {} | SKIPPED: {} | BYTES: {}",
                        target.name, stats.files_copied, stats.files_skipped, stats.bytes_transferred
                    );
                }
            }
            Err(e) => {
                println!("{RED}[SYNC FAILED: {}]{RESET}", e);
            }
        }
    }

    let elapsed = timer.elapsed();
    let total_mb = total_bytes as f64 / (1024.0 * 1024.0);

    println!("\n{GRAY}----------------------------------------------------------------------{RESET}");
    println!("{GREEN}{BOLD} [✓] DATA REPLICATION COMPLETED SUCCESSFULLY!{RESET}");
    println!(" {CYAN}Total Clusters Secured :{RESET} {WHITE}{}{RESET}", targets.len());
    println!(" {CYAN}Files Synced           :{RESET} {GREEN}{}{RESET} new/modified ({GRAY}{} cached{RESET})", total_copied, total_skipped);
    println!(" {CYAN}Bandwidth Transferred   :{RESET} {YELLOW}{:.2} MB{RESET}", total_mb);
    println!(" {CYAN}Execution Time          :{RESET} {WHITE}{:.2?}{RESET}", elapsed);
    println!(" {CYAN}Storage Vault           :{RESET} {WHITE}{}{RESET}", dest_base.display());
    println!(" {CYAN}Audit Log Dispatched    :{RESET} {GRAY}{}{RESET}", log_file_path.display());
    println!("{GRAY}----------------------------------------------------------------------{RESET}\n");

    print!("{YELLOW}>> Press [ENTER] to disconnect... {RESET}");
    let _ = io::stdout().flush();
    let mut _end = String::new();
    let _ = io::stdin().read_line(&mut _end);
}
