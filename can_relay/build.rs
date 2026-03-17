fn main() {
    embuild::espidf::sysenv::output();

    // Capture build timestamp for RTC initialization
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", now);
}
