fn main() {
    embuild::espidf::sysenv::output();

    // Declare the ESP-IDF component cfg flags so rustc doesn't warn about
    // "unexpected cfg condition name" when the managed components aren't built.
    println!("cargo::rustc-check-cfg=cfg(esp_idf_comp_mdns_enabled)");
    println!("cargo::rustc-check-cfg=cfg(esp_idf_comp_espressif__mdns_enabled)");
    println!("cargo::rustc-check-cfg=cfg(esp_idf_comp_espressif__esp_websocket_client_enabled)");
}
