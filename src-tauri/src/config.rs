macro_rules! build_env {
    ($name:literal, $default:expr) => {
        match option_env!($name) {
            Some(value) => value,
            None => $default,
        }
    };
}

pub const CDN_BASE_URL: &str =
    build_env!("INFERNUM_CDN_BASE_URL", "https://infernum-packs.pages.dev");

pub const MSA_ENABLED: bool = match option_env!("INFERNUM_MSA_ENABLED") {
    Some(v) => !matches!(v.as_bytes(), [b'0'] | [b'f', b'a', b'l', b's', b'e']),
    None => true,
};

pub const DISCORD_INVITE_URL: &str = "https://discord.gg/RZuNWDvDQg";
pub const TWITTER_URL: &str = "https://x.com/Infernum_Studio";

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

pub const APP_IDENTIFIER: &str = "studio.infernum.launcher";

pub const EVENTS_PUBLIC_KEY: &str = build_env!(
    "INFERNUM_EVENTS_PUBKEY",
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhCQUM3OTAxNkUzMzkwMEQKUldRTmtETnVBWG1zaS9FQ1VrUnN6UmhUQ2lvampwQlV6OW9jUjdYMXVJazIyeHJtRzJmdnV3aEgK"
);

const _: () = assert!(
    cfg!(debug_assertions) || !EVENTS_PUBLIC_KEY.is_empty(),
    "se necesita la clave pública para una release de events.json: \
     usa EVENTS_PUBLIC_KEY en config.rs o compila con INFERNUM_EVENTS_PUBKEY"
);

pub fn events_url() -> String {
    if cfg!(debug_assertions)
        && let Ok(url) = std::env::var("INFERNUM_EVENTS_URL")
    {
        return url;
    }
    format!("{}/events.json", CDN_BASE_URL.trim_end_matches('/'))
}

pub fn events_public_key() -> String {
    if cfg!(debug_assertions)
        && let Ok(key) = std::env::var("INFERNUM_EVENTS_PUBKEY")
    {
        return key;
    }
    EVENTS_PUBLIC_KEY.to_string()
}

pub fn user_agent() -> String {
    format!(
        "infernum-launcher/{} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    )
}
