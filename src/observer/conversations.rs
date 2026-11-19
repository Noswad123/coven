use super::*;
use config::ConversationConfig;

#[derive(Default)]
pub(super) struct Conversations(Vec<(String, ConversationConfig)>);

impl Conversations {
    pub(super) fn new(config: &Config, epoch: &str) -> Result<Self> {
        // Full validated configuration plus per-process entropy invalidates every
        // reference on configuration changes/restarts, including ACL-only changes.
        let generation = serde_json::to_string(config).map_err(|_| Error("invalid_config"))?;
        Ok(Self(
            config
                .conversations
                .iter()
                .map(|target| {
                    (
                        digest(&[
                            epoch,
                            &generation,
                            &target.id,
                            &target.team,
                            &target.expected_runtime,
                            &target.session,
                        ]),
                        target.clone(),
                    )
                })
                .collect(),
        ))
    }

    pub(super) fn list(&self, login: &str) -> Value {
        json!({"contract":"coven.conversations.v1", "conversations":self.0.iter()
            .filter(|(_, target)| target.allowed_logins.iter().any(|l| l == login))
            .map(|(id, target)| json!({"id":id,"label":target.label,"team":target.team,"host":"Local native"}))
            .collect::<Vec<_>>()})
    }

    pub(super) fn read(&self, login: &str, id: &str) -> Result<Value> {
        // Never connect, select or stat an endpoint before resource/team admission.
        let (_, target) = self
            .0
            .iter()
            .find(|(reference, target)| {
                reference == id && target.allowed_logins.iter().any(|l| l == login)
            })
            .ok_or(Error("invalid_reference"))?;
        let mut value = crate::session_conversations::read(
            &target.native_socket,
            &target.expected_runtime,
            &target.session,
        )?;
        value["contract"] = json!("coven.conversations.v1");
        value["id"] = json!(id);
        value["team"] = json!(target.team);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_schema_is_closed_and_bounded() {
        let base = json!({"listen":"127.0.0.1:7341","public_origin":"https://example.invalid",
            "allowed_logins":["alice"],"endpoints":[{"id":"fleet","label":"Fleet","socket":"/absent/f.sock"}],
            "conversations":[{"id":"one","label":"One","team":"research","allowed_logins":["alice"],
                "native_socket":"/private/native.sock","expected_runtime":"11111111-2222-4333-8444-555555555555","session":"ses_one"}]});
        for (key, value) in [
            ("id", json!("../secret")),
            ("label", json!("\n")),
            ("team", json!("")),
            ("allowed_logins", json!([])),
            ("allowed_logins", json!(["alice", "alice"])),
            ("native_socket", json!("/private/../native.sock")),
            ("native_socket", json!(format!("/{}", "x".repeat(100)))),
            ("expected_runtime", json!("not-a-uuid")),
            ("session", json!("ses_")),
        ] {
            let mut bad = base.clone();
            bad["conversations"][0][key] = value;
            assert!(
                serde_json::from_value::<Config>(bad)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{key}"
            );
        }
        let mut unknown = base.clone();
        unknown["conversations"][0]["ssh"] = json!({});
        assert!(serde_json::from_value::<Config>(unknown).is_err());
        let mut missing = base.clone();
        missing["conversations"][0]
            .as_object_mut()
            .unwrap()
            .remove("session");
        assert!(serde_json::from_value::<Config>(missing).is_err());
        let mut duplicate = base.clone();
        duplicate["conversations"]
            .as_array_mut()
            .unwrap()
            .push(base["conversations"][0].clone());
        assert!(
            serde_json::from_value::<Config>(duplicate)
                .unwrap()
                .validate()
                .is_err()
        );
        let mut too_many = base.clone();
        too_many["conversations"] = json!(vec![base["conversations"][0].clone(); 65]);
        assert_eq!(
            serde_json::from_value::<Config>(too_many)
                .unwrap()
                .validate()
                .unwrap_err()
                .code(),
            "conversation_limit"
        );
    }
    #[test]
    fn acl_and_full_configuration_generation_fence_references() {
        let mut config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341",
            "public_origin":"https://example.invalid","allowed_logins":["alice","bob"],
            "endpoints":[{"id":"fleet","label":"Fleet","socket":"/unavailable/fleet.sock"}],
            "conversations":[{"id":"secret-id","label":"Registered","team":"research",
                "allowed_logins":["alice"],"native_socket":"/unavailable/native.sock",
                "expected_runtime":"11111111-2222-4333-8444-555555555555","session":"ses_secret"}]})).unwrap();
        config.validate().unwrap();
        let registry = Conversations::new(&config, "epoch").unwrap();
        let list = registry.list("alice");
        let id = list["conversations"][0]["id"].as_str().unwrap();
        assert!(
            registry.list("bob")["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            registry.read("bob", id).unwrap_err().code(),
            "invalid_reference"
        );
        assert_ne!(
            id,
            Conversations::new(&config, "other-epoch")
                .unwrap()
                .list("alice")["conversations"][0]["id"]
        );
        config.endpoints[0].label = "Changed unrelated config".into();
        assert_ne!(
            id,
            Conversations::new(&config, "epoch").unwrap().list("alice")["conversations"][0]["id"]
        );
        config.conversations[0].allowed_logins = vec!["outsider".into()];
        assert!(config.validate().is_err());
        config.conversations[0].allowed_logins = vec!["alice".into()];
        config.conversations[0].native_socket = "ssh://remote/socket".into();
        assert!(config.validate().is_err());
    }
}
