fn main() {
    // The Open-SIMH runtime is embedded with include_bytes! and
    // simh_frontpanel.dll is loaded dynamically at runtime. Keep these explicit
    // rerun hints so replacing the validated bundle always rebuilds RusTair.
    println!("cargo:rerun-if-changed=SIMH-backend/altair.exe");
    println!("cargo:rerun-if-changed=SIMH-backend/altairz80.exe");
    println!("cargo:rerun-if-changed=SIMH-backend/simh_frontpanel.dll");
}
