use crate::{Error, Session, timed};
use prost::Message;
use std::collections::{BTreeMap, BTreeSet};
use steamroom::{
    client::msg::ClientMsg,
    depot::AppId,
    generated::{self, CMsgClientPicsProductInfoRequest, CMsgClientPicsProductInfoResponse},
    messages::{EMsg, header::PacketHeader},
    types::key_value::{KeyValue, KvValue},
};

#[derive(Clone, Default)]
pub struct Game {
    pub app_id: u32,
    pub name: String,
    pub platforms: Vec<String>,
    pub languages: Vec<String>,
    pub depots: Vec<Depot>,
    pub branches: Vec<Branch>,
    pub(crate) branch_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Branch {
    pub name: String,
    pub password_required: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Depot {
    pub depot_id: u32,
    pub source_app_id: u32,
    pub name: String,
    pub platform: String,
    pub language: String,
    pub manifests: BTreeMap<String, u64>,
    pub sizes: BTreeMap<String, u64>,
    pub(crate) encrypted_manifests: BTreeMap<String, String>,
}

fn children(node: Option<&KeyValue>) -> impl Iterator<Item = (&String, &KeyValue)> {
    node.and_then(|node| match &node.value {
        KvValue::Children(map) => Some(map.iter()),
        _ => None,
    })
    .into_iter()
    .flatten()
}
fn text(node: Option<&KeyValue>) -> String {
    node.and_then(KeyValue::as_str)
        .unwrap_or_default()
        .to_owned()
}
fn number(node: Option<&KeyValue>) -> Option<u64> {
    node.and_then(|node| {
        node.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| node.as_i32().and_then(|n| n.try_into().ok()))
            .or_else(|| node.as_u64())
    })
}
fn parse_app(data: &[u8]) -> Result<KeyValue, Error> {
    let text = std::str::from_utf8(data)
        .map_err(|_| Error::Metadata)?
        .trim_end_matches('\0');
    KeyValue::from_text(text).map_err(|_| Error::Metadata)
}

pub(crate) fn parse_game(app_id: u32, kv: &KeyValue) -> Game {
    let common = kv.get("common");
    let depots = kv.get("depots");
    let mut game = Game {
        app_id,
        name: text(common.and_then(|v| v.get("name"))),
        platforms: text(common.and_then(|v| v.get("oslist")))
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        languages: children(common.and_then(|v| v.get("supported_languages")))
            .map(|(name, _)| name.clone())
            .collect(),
        depots: Vec::new(),
        branches: children(depots.and_then(|v| v.get("branches")))
            .map(|(name, branch)| Branch {
                name: name.clone(),
                password_required: number(branch.get("pwdrequired")) == Some(1),
            })
            .collect(),
        branch_hashes: BTreeMap::new(),
    };
    for (id, depot) in children(depots) {
        let Ok(depot_id) = id.parse() else {
            continue;
        };
        let config = depot.get("config");
        let mut entry = Depot {
            depot_id,
            source_app_id: number(depot.get("depotfromapp"))
                .and_then(|n| n.try_into().ok())
                .unwrap_or(app_id),
            name: text(depot.get("name")),
            platform: text(config.and_then(|v| v.get("oslist"))),
            language: text(config.and_then(|v| v.get("language"))),
            ..Default::default()
        };
        for (branch, manifest) in children(depot.get("manifests")) {
            if let Some(gid) = number(manifest.get("gid")).or_else(|| number(Some(manifest))) {
                entry.manifests.insert(branch.clone(), gid);
            }
            if let Some(size) = number(manifest.get("size")) {
                entry.sizes.insert(branch.clone(), size);
            }
        }
        for (branch, manifest) in children(depot.get("encryptedmanifests")) {
            let gid = text(
                manifest
                    .get("encrypted_gid_2")
                    .or_else(|| manifest.get("encrypted_gid")),
            );
            if !gid.is_empty() {
                entry.encrypted_manifests.insert(branch.clone(), gid);
            }
        }
        game.depots.push(entry);
    }
    if !game.branches.iter().any(|branch| branch.name == "public") {
        game.branches.insert(
            0,
            Branch {
                name: "public".into(),
                password_required: false,
            },
        );
    }
    game
}

impl Session {
    pub(crate) async fn send_request(&self, emsg: EMsg, body: &[u8]) -> Result<(), Error> {
        let mut msg = ClientMsg::with_body(emsg, body);
        {
            let wire = self.wire.lock().unwrap_or_else(|e| e.into_inner());
            msg.header.steamid = wire.steam_id;
            msg.header.client_sessionid = wire.session_id;
        }
        timed(self.client.send_msg(&msg)).await
    }

    pub(crate) async fn receive(&self, expected: EMsg) -> Result<Vec<bytes::Bytes>, Error> {
        timed(async {
            loop {
                let incoming = self.client.recv_msg().await?;
                if incoming.emsg == EMsg::CLIENT_LOGGED_OFF {
                    return Err(steamroom::Error::Connection(
                        steamroom::error::ConnectionError::LogonFailed(
                            steamroom::enums::EResultError::InvalidPassword,
                        ),
                    ));
                }
                if incoming.emsg == expected {
                    return Ok(vec![incoming.body]);
                }
                if incoming.emsg == EMsg::MULTI {
                    let mut matches = Vec::new();
                    for packet in steamroom::client::multi::unpack_multi(&incoming.body)? {
                        if let Ok(PacketHeader::Protobuf { header, body }) =
                            PacketHeader::parse(&packet)
                        {
                            if header.emsg == expected {
                                matches.push(body);
                            }
                        }
                    }
                    if !matches.is_empty() {
                        return Ok(matches);
                    }
                }
            }
        })
        .await
    }

    async fn product_info(
        &self,
        request: CMsgClientPicsProductInfoRequest,
    ) -> Result<Vec<CMsgClientPicsProductInfoResponse>, Error> {
        self.send_request(
            EMsg::CLIENT_PICS_PRODUCT_INFO_REQUEST,
            &request.encode_to_vec(),
        )
        .await?;
        let mut responses = Vec::new();
        loop {
            let mut pending = false;
            for body in self
                .receive(EMsg::CLIENT_PICS_PRODUCT_INFO_RESPONSE)
                .await?
            {
                let response = CMsgClientPicsProductInfoResponse::decode(&*body)
                    .map_err(|_| Error::Metadata)?;
                pending = response.response_pending.unwrap_or(false);
                responses.push(response);
            }
            if !pending {
                return Ok(responses);
            }
        }
    }

    pub async fn game(&self, app_id: u32) -> Result<Game, Error> {
        let kv = self.app_kv(app_id).await?;
        let mut game = parse_game(app_id, &kv);
        // Shared depots inherit their manifests from another app.
        for depot in &mut game.depots {
            if depot.source_app_id != app_id {
                let source = parse_game(
                    depot.source_app_id,
                    &self.app_kv(depot.source_app_id).await?,
                );
                if let Some(shared) = source
                    .depots
                    .into_iter()
                    .find(|d| d.depot_id == depot.depot_id)
                {
                    depot.manifests = shared.manifests;
                    depot.sizes = shared.sizes;
                    depot.encrypted_manifests = shared.encrypted_manifests;
                }
            }
        }
        Ok(game)
    }

    async fn app_kv(&self, app_id: u32) -> Result<KeyValue, Error> {
        let tokens = timed(self.client.pics_get_access_tokens(&[AppId(app_id)])).await?;
        let token = tokens
            .iter()
            .find(|t| t.app_id.0 == app_id)
            .map_or(0, |t| t.token);
        let responses = self
            .product_info(CMsgClientPicsProductInfoRequest {
                apps: vec![generated::c_msg_client_pics_product_info_request::AppInfo {
                    appid: Some(app_id),
                    access_token: Some(token),
                    ..Default::default()
                }],
                meta_data_only: Some(false),
                ..Default::default()
            })
            .await?;
        let data = responses
            .into_iter()
            .flat_map(|r| r.apps)
            .find(|a| a.appid == Some(app_id))
            .and_then(|a| a.buffer)
            .ok_or(Error::Metadata)?;
        parse_app(&data)
    }

    /// Enumerate licenses → packages → app IDs, then retain products of type game.
    /// Requests are serialized and bounded to one product at a time; no store API
    /// key, public profile, or first-page shortcut is involved.
    pub async fn owned_games(&self) -> Result<Vec<Game>, Error> {
        if self
            .wire
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .licenses
            .is_none()
        {
            self.receive(EMsg::CLIENT_LICENSE_LIST).await?;
        }
        let licenses = self
            .wire
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .licenses
            .clone()
            .ok_or(Error::Metadata)?;
        let mut app_ids = BTreeSet::new();
        for (package_id, mut access_token) in licenses {
            if access_token == 0 {
                access_token =
                    timed(self.client.pics_get_package_access_tokens(&[
                        steamroom::depot::PackageId(package_id),
                    ]))
                    .await?
                    .into_iter()
                    .find(|(id, _)| id.0 == package_id)
                    .map_or(0, |(_, token)| token);
            }
            let responses = self
                .product_info(CMsgClientPicsProductInfoRequest {
                    packages: vec![
                        generated::c_msg_client_pics_product_info_request::PackageInfo {
                            packageid: Some(package_id),
                            access_token: Some(access_token),
                        },
                    ],
                    meta_data_only: Some(false),
                    ..Default::default()
                })
                .await?;
            let data = responses
                .into_iter()
                .flat_map(|r| r.packages)
                .find(|package| package.packageid == Some(package_id))
                .and_then(|package| package.buffer)
                .ok_or(Error::Metadata)?;
            // PICS packages prefix binary KV with a 32-bit serialization version.
            let kv = KeyValue::from_binary(data.get(4..).ok_or(Error::Metadata)?)
                .map_err(|_| Error::Metadata)?;
            for (_, id) in children(kv.get("appids")) {
                if let Some(id) = number(Some(id)).and_then(|n| u32::try_from(n).ok()) {
                    app_ids.insert(id);
                }
            }
        }
        let mut games = Vec::new();
        for app_id in app_ids {
            let kv = self.app_kv(app_id).await?;
            if text(kv.get("common").and_then(|v| v.get("type"))).eq_ignore_ascii_case("game") {
                games.push(parse_game(app_id, &kv));
            }
        }
        games.sort_by_cached_key(|game| game.name.to_lowercase());
        Ok(games)
    }

    pub async fn unlock_branch(
        &self,
        game: &mut Game,
        branch: &str,
        password: &str,
    ) -> Result<(), Error> {
        if password.is_empty() {
            return Ok(());
        }
        let request = generated::CMsgClientCheckAppBetaPassword {
            app_id: Some(game.app_id),
            betapassword: Some(password.into()),
            ..Default::default()
        };
        self.send_request(EMsg(5450), &request.encode_to_vec())
            .await?;
        let replies = self.receive(EMsg(5451)).await?;
        let response = generated::CMsgClientCheckAppBetaPasswordResponse::decode(&*replies[0])
            .map_err(|_| Error::Metadata)?;
        if response.eresult != Some(1) {
            return Err(Error::AccessDenied);
        }
        let hash = response
            .betapasswords
            .iter()
            .find(|p| p.betaname.as_deref() == Some(branch))
            .and_then(|p| p.betapassword.as_deref())
            .ok_or(Error::AccessDenied)?;
        let key =
            zeroize::Zeroizing::new(steamroom::util::hex::decode(hash).ok_or(Error::Metadata)?);
        for depot in &mut game.depots {
            if let Some(encrypted) = depot.encrypted_manifests.get(branch) {
                let data = steamroom::util::hex::decode(encrypted).ok_or(Error::Metadata)?;
                let decrypted = steamroom::crypto::symmetric_decrypt_ecb(&data, &key)
                    .map_err(|_| Error::AccessDenied)?;
                let gid = u64::from_le_bytes(
                    decrypted
                        .get(..8)
                        .ok_or(Error::Metadata)?
                        .try_into()
                        .map_err(|_| Error::Metadata)?,
                );
                depot.manifests.insert(branch.into(), gid);
            }
        }
        game.branch_hashes.insert(branch.into(), hash.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_preserves_shared_language_depots_and_nonpublic_branches() {
        let kv = KeyValue::from_text(r#""appinfo" { "common" { "name" "VN" "oslist" "windows,linux" "supported_languages" { "japanese" { "supported" "true" } } } "depots" { "11" { "name" "Windows" "config" { "oslist" "windows" } "manifests" { "public" { "gid" "123" "size" "45" } } } "12" { "depotfromapp" "99" "config" { "language" "japanese" } } "branches" { "beta" { "pwdrequired" "1" } } } }"#).unwrap();
        let game = parse_game(10, &kv);
        assert_eq!(game.depots[0].manifests["public"], 123);
        assert_eq!(game.depots[0].sizes["public"], 45);
        assert_eq!(game.depots[1].source_app_id, 99);
        assert_eq!(game.depots[1].language, "japanese");
        assert!(
            game.branches
                .iter()
                .any(|b| b.name == "beta" && b.password_required)
        );
        assert_eq!(game.platforms, ["windows", "linux"]);
    }
}
