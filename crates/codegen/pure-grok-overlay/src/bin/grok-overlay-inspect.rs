use std::env;

fn main() {
    let document = env::var("GROK_OVERLAY_CONFIG")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.parse::<toml::Value>().ok());
    let runtime =
        xai_grok_overlay::OverlayRuntime::from_toml(document.as_ref(), |key| env::var(key).ok())
            .unwrap_or_else(|error| {
                eprintln!("overlay configuration error: {error}");
                std::process::exit(2);
            });

    println!("mode: {:?}", runtime.policy().mode);
    println!("auth: {:?}", runtime.auth_policy());
    println!("entitlement: {:?}", runtime.entitlement());
    println!("capabilities: {:?}", runtime.capabilities());
    println!("update_source: {:?}", runtime.update_source());
}
