// Build script to ensure LIBCLANG_PATH symlink exists for environments without libclang-dev
use std::path::Path;
fn main() {
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
        "/usr/lib/x86_64-linux-gnu/libclang-21.so.1",
        "/usr/lib/llvm-21/lib/libclang.so.1",
        "/usr/lib/llvm-18/lib/libclang.so.1",
        "/usr/lib/llvm-21/lib/libclang-21.so.1",
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
    // Fallback: search via find (best effort, don't fail build)
    println!("cargo:warning=libclang.so not found, build may fail. Run ./setup.sh or install libclang-dev");
}
