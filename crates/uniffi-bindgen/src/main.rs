fn main() {
    // Use UniFFI's generic CLI rather than a language-specific wrapper: the
    // Android build only needs Kotlin today, but keeping one entry point means
    // future bindings do not need another binary crate.
    uniffi::uniffi_bindgen_main()
}
