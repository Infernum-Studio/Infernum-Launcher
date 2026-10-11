use base64::Engine;
use serde::Serialize;

use super::{Account, AccountKind};
use crate::error::AppResult;
use crate::paths::Paths;

const DEFAULT_SKINS: [(&str, bool); 18] = [
    (
        "46acd06e8483b176e8ea39fc12fe105eb3a2a4970f5100057e9d84d4b60bdfa7",
        true,
    ),
    (
        "6ac6ca262d67bcfb3dbc924ba8215a18195497c780058a5749de674217721892",
        true,
    ),
    (
        "fece7017b1bb13926d1158864b283b8b930271f80a90482f174cca6a17e88236",
        true,
    ),
    (
        "226c617fde5b1ba569aa08bd2cb6fd84c93337532a872b3eb7bf66bdd5b395f8",
        true,
    ),
    (
        "7cb3ba52ddd5cc82c0b050c3f920f87da36add80165846f479079663805433db",
        true,
    ),
    (
        "6c160fbd16adbc4bff2409e70180d911002aebcfa811eb6ec3d1040761aea6dd",
        true,
    ),
    (
        "d5c4ee5ce20aed9e33e866c66caa37178606234b3721084bf01d13320fb2eb3f",
        true,
    ),
    (
        "b66bc80f002b10371e2fa23de6f230dd5e2f3affc2e15786f65bc9be4c6eb71a",
        true,
    ),
    (
        "eee522611005acf256dbd152e992c60c0bb7978cb0f3127807700e478ad97664",
        true,
    ),
    (
        "1abc803022d8300ab7578b189294cce39622d9a404cdc00d3feacfdf45be6981",
        false,
    ),
    (
        "4c05ab9e07b3505dc3ec11370c3bdce5570ad2fb2b562e9b9dd9cf271f81aa44",
        false,
    ),
    (
        "daf3d88ccb38f11f74814e92053d92f7728ddb1a7955652a60e30cb27ae6659f",
        false,
    ),
    (
        "e5cdc3243b2153ab28a159861be643a4fc1e3c17d291cdd3e57a7f370ad676f3",
        false,
    ),
    (
        "dc0fcfaf2aa040a83dc0de4e56058d1bbb2ea40157501f3e7d15dc245e493095",
        false,
    ),
    (
        "90e75cd429ba6331cd210b9bd19399527ee3bab467b5a9f61cb8a27b177f6789",
        false,
    ),
    (
        "31f477eb1a7beee631c2ca64d06f8f68fa93a3386d04452ab27f43acdf1b60cb",
        false,
    ),
    (
        "a3bd16079f764cd541e072e888fe43885e711f98658323db0f9a6045da91ee7a",
        false,
    ),
    (
        "f5dddb41dcafef616e959c2817808e0be741c89ffbfed39134a13e75b811863d",
        false,
    ),
];

fn java_uuid_hash(uuid: uuid::Uuid) -> i32 {
    let (hi, lo) = uuid.as_u64_pair();
    let hilo = hi ^ lo;
    ((hilo >> 32) as i32) ^ (hilo as i32)
}

pub fn default_skin(uuid: uuid::Uuid) -> (&'static str, bool) {
    DEFAULT_SKINS[java_uuid_hash(uuid).rem_euclid(DEFAULT_SKINS.len() as i32) as usize]
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinData {
    pub data_url: String,
    pub slim: bool,
}

pub async fn get(paths: &Paths, account: &Account) -> AppResult<SkinData> {
    let uuid = uuid::Uuid::parse_str(&account.id).unwrap_or_default();
    let fallback = default_url(uuid);
    let (url, slim) = match account.kind {
        AccountKind::Microsoft => super::microsoft::skin(&account.id)
            .await
            .unwrap_or_else(|| fallback.clone()),
        AccountKind::Offline => fallback.clone(),
    };
    let (bytes, slim) = match texture(paths, &url).await {
        Ok(bytes) => (bytes, slim),
        Err(err) if url != fallback.0 => {
            log::warn!("skin download failed ({err}); using the default skin");
            (texture(paths, &fallback.0).await?, fallback.1)
        }
        Err(err) => return Err(err),
    };
    Ok(SkinData {
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ),
        slim,
    })
}

async fn texture(paths: &Paths, url: &str) -> AppResult<Vec<u8>> {
    let cache = paths.cache_dir().join("skins").join(format!(
        "{}.png",
        &crate::net::sha512_hex(url.as_bytes())[..32]
    ));
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        return Ok(bytes);
    }
    let bytes = crate::net::fetch_bytes(url, 256 * 1024).await?;
    if !bytes.starts_with(b"\x89PNG") {
        return Err(crate::error::AppError::new(
            "invalid_data",
            "skin is not a PNG",
        ));
    }
    crate::paths::write_atomic(&cache, &bytes).await?;
    Ok(bytes)
}

fn default_url(uuid: uuid::Uuid) -> (String, bool) {
    let (key, slim) = default_skin(uuid);
    (
        format!("https://textures.minecraft.net/texture/{key}"),
        slim,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_hash_matches_java() {
        let uuid = uuid::Uuid::from_u64_pair(0x0000_0001_0000_0002, 0x0000_0003_0000_0004);
        assert_eq!(java_uuid_hash(uuid), 4);
        let uuid = uuid::Uuid::from_u64_pair(0xffff_ffff_0000_0000, 0);
        assert_eq!(java_uuid_hash(uuid), -1);
        assert_eq!(
            default_skin(uuid),
            DEFAULT_SKINS[(-1i32).rem_euclid(18) as usize]
        );
    }
}
