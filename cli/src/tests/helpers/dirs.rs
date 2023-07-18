lazy_static! {
    static ref ROOT_DIR: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned();
    static ref FIXTURES_DIR: PathBuf = ROOT_DIR.join("test").join("fixtures");
    static ref HEADER_DIR: PathBuf = ROOT_DIR.join("lib").join("include");
    static ref GRAMMARS_DIR: PathBuf = ROOT_DIR.join("test").join("fixtures").join("grammars");
    static ref SCRATCH_DIR: PathBuf = {
        // https://doc.rust-lang.org/reference/conditional-compilation.html
        let arch = if cfg!(target_arch = "x86") {
            "x86"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "mips") {
            "mips"
        } else if cfg!(target_arch = "powerpc") {
            "powerpc"
        } else if cfg!(target_arch = "powerpc64") {
            "powerpc64"
        } else if cfg!(target_arch = "arm") {
            "arm"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "unknown"
        };
        let os = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "ios") {
            "ios"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "android") {
            "android"
        } else if cfg!(target_os = "freebsd") {
            "freebsd"
        } else if cfg!(target_os = "dragonfly") {
            "dragonfly"
        } else if cfg!(target_os = "netbsd") {
            "netbsd"
        } else if cfg!(target_os = "openbsd") {
            "openbsd"
        } else {
            "unknown"
        };
        let env = if cfg!(target_env = "gnu") {
            "gnu"
        } else if cfg!(target_env = "msvc") {
            "msvc"
        } else if cfg!(target_env = "musl") {
            "musl"
        } else if cfg!(target_env = "sgx") {
            "sgx"
        } else {
            "unknown"
        };
        let endian = if cfg!(target_endian = "little") {
            "little"
        } else if cfg!(target_endian = "big") {
            "big"
        } else {
            "unknown"
        };
        let vendor = if cfg!(target_vendor = "apple") {
            "apple"
        } else if cfg!(target_vendor = "fortanix") {
            "fortanix"
        } else if cfg!(target_vendor = "pc") {
            "pc"
        } else {
            "unknown"
        };

        let machine = format!("{}-{}-{}-{}-{}", arch, os, env, endian, vendor);
        let result = ROOT_DIR.join("target").join("scratch").join(machine);
        println!("SCRATCH_DIR: {:?}", result);
        fs::create_dir_all(&result).unwrap();
        result
    };
}
