use std::env;

fn main() {
    let runtime = if let Some(path) = env::var_os("GROK_OVERLAY_CONFIG") {
        let document = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse::<toml::Value>().ok());
        xai_grok_overlay::OverlayRuntime::from_toml(document.as_ref(), |key| env::var(key).ok())
    } else {
        xai_grok_overlay::load_runtime()
    }
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
