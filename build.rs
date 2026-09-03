// Build script to ensure LIBCLANG_PATH symlink exists for environments without libclang-dev
use std::path::Path;
fn main() {
    // Windows doesn't need libclang.so
    if cfg!(windows) {
        return;
    }
    let libclang_dir = Path::new(".libclang");
    let target = libclang_dir.join("libclang.so");
    if target.exists() {
        return;
    }
    // Try to create directory
    let _ = std::fs::create_dir_all(libclang_dir);
    // Search for libclang
    let candidates = [
        "/usr/lib/x86_64-linux-gnu/libclang-21.so.21",
        "/usr/lib/x86_64-linux-gnu/libclang-18.so.18",
        "/usr/lib/x86_64-linux-gnu/libclang-17.so.1",
        "/usr/lib/x86_64-linux-gnu/libclang-15.so.1",
        "/usr/lib/x86_64-linux-gnu/libclang-14.so.1",
        "/usr/lib/llvm-21/lib/libclang.so.1",
        "/usr/lib/llvm-18/lib/libclang.so.1",
        "/usr/lib/llvm-17/lib/libclang.so.1",
        "/usr/lib/llvm-15/lib/libclang.so.1",
        "/usr/lib/llvm-14/lib/libclang.so.1",
        "/usr/lib/llvm-21/lib/libclang-21.so.1",
        "/usr/lib/llvm-18/lib/libclang.so.1",
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                let _ = std::fs::remove_file(&target);
                if symlink(c, &target).is_ok() {
                    println!("cargo:warning=Created symlink {} -> {}", c, target.display());
                }
            }
            return;
        }
    }
    // Try llvm-config
    for cfg in &[
        "llvm-config",
        "llvm-config-21",
        "llvm-config-18",
        "llvm-config-17",
        "llvm-config-15",
        "llvm-config-14",
    ] {
        if let Ok(output) = std::process::Command::new(cfg).arg("--libdir").output() {
            if output.status.success() {
                let libdir = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let candidate = Path::new(&libdir).join("libclang.so");
                if candidate.exists() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::symlink;
                        let _ = std::fs::remove_file(&target);
                        let _ = symlink(&candidate, &target);
                    }
                    return;
                }
                let candidate_so1 = Path::new(&libdir).join("libclang.so.1");
                if candidate_so1.exists() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::symlink;
                        let _ = std::fs::remove_file(&target);
                        let _ = symlink(&candidate_so1, &target);
                    }
                    return;
                }
            }
        }
    }
    // Fallback: search via find (best effort, don't fail build)
    // Only warn on Unix
    if cfg!(unix) {
        println!("cargo:warning=libclang.so not found, build may fail. Run ./setup.sh or install libclang-dev");
    }
}
