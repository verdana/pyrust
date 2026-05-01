fn main() {
    // Generate CLSID from a UUID v5 (namespace-based) for deterministic build
    println!("cargo:rustc-env=PYRUST_CLSID={{D4B3C2A1-9F8E-7D6C-5B4A-3928174655AA}}");
    println!("cargo:rustc-env=PYRUST_PROFILE_GUID={{E5C4B3A2-0F9E-8D7C-6B5A-4938271655BB}}");
}
