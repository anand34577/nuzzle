//! Keeps tokens out of `settings.json`. Header and environment values of connectors and MCP servers (API keys,
//! bearer tokens) go to the Windows Credential Manager; the file only holds a reference such as
//! `keyring:mcp:github:env:TOKEN`. In memory the settings always hold the real values, so nothing else changes.
//!
//! If the credential store can't be used the value stays in the file (and a warning is logged): losing a working
//! connector would be worse than the exposure that existed before.

use std::collections::BTreeSet;

use crate::settings::Settings;

const SERVICE: &str = "dev.companion.desktop";
const PREFIX: &str = "keyring:";

/// Where secrets live. The real one is the OS credential store; tests use a map.
pub trait Store {
    fn put(&mut self, id: &str, value: &str) -> Result<(), String>;
    fn get(&self, id: &str) -> Option<String>;
    fn remove(&mut self, id: &str);
}

pub struct OsStore;

impl Store for OsStore {
    fn put(&mut self, id: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, id).and_then(|e| e.set_password(value)).map_err(|e| e.to_string())
    }
    fn get(&self, id: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, id).ok()?.get_password().ok()
    }
    fn remove(&mut self, id: &str) {
        if let Ok(e) = keyring::Entry::new(SERVICE, id) {
            let _ = e.delete_credential();
        }
    }
}

/// Every secret field of the settings: (credential id, mutable value).
fn fields(s: &mut Settings) -> Vec<(String, &mut String)> {
    let mut out = vec![];
    for w in &mut s.webhooks {
        for (k, v) in &mut w.headers {
            out.push((format!("hook:{}:header:{}", w.name, k), v));
        }
    }
    for m in &mut s.mcp_servers {
        for (k, v) in &mut m.env {
            out.push((format!("mcp:{}:env:{}", m.name, k), v));
        }
        for (k, v) in &mut m.headers {
            out.push((format!("mcp:{}:header:{}", m.name, k), v));
        }
    }
    out
}

/// The credential ids the settings refer to, with each secret value replaced by a reference. `ids_before` are the
/// ones the file referred to before; those no longer used are deleted. Returns the set now in use.
pub fn protect(s: &mut Settings, store: &mut dyn Store, ids_before: &BTreeSet<String>) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    for (id, v) in fields(s) {
        if let Some(existing) = v.strip_prefix(PREFIX) {
            used.insert(existing.to_string()); // already a reference: still in use, so it must not be deleted
            continue;
        }
        if v.is_empty() {
            continue;
        }
        match store.put(&id, v) {
            Ok(()) => {
                used.insert(id.clone());
                *v = format!("{PREFIX}{id}");
            }
            Err(e) => crate::log::warn(&format!("couldn't move a secret to the credential store ({e}); keeping it in the file")),
        }
    }
    for gone in ids_before.difference(&used) {
        store.remove(gone);
    }
    used
}

/// Replace references by the stored values. Returns the ids found and a list of ones the store no longer has.
pub fn reveal(s: &mut Settings, store: &dyn Store) -> (BTreeSet<String>, Vec<String>) {
    let (mut found, mut missing) = (BTreeSet::new(), vec![]);
    for (_, v) in fields(s) {
        let Some(id) = v.strip_prefix(PREFIX).map(str::to_string) else { continue };
        match store.get(&id) {
            Some(real) => {
                found.insert(id);
                *v = real;
            }
            None => {
                missing.push(id);
                v.clear();
            }
        }
    }
    (found, missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::settings::{McpServerCfg, Webhook};

    #[derive(Default)]
    pub struct MapStore(pub HashMap<String, String>);

    impl Store for MapStore {
        fn put(&mut self, id: &str, value: &str) -> Result<(), String> {
            self.0.insert(id.into(), value.into());
            Ok(())
        }
        fn get(&self, id: &str) -> Option<String> {
            self.0.get(id).cloned()
        }
        fn remove(&mut self, id: &str) {
            self.0.remove(id);
        }
    }

    fn with_secrets() -> Settings {
        Settings {
            webhooks: vec![Webhook {
                name: "notify".into(),
                url: "https://x".into(),
                headers: [("Authorization".to_string(), "Bearer abc".to_string())].into(),
                ..Default::default()
            }],
            mcp_servers: vec![McpServerCfg {
                name: "gh".into(),
                env: [("TOKEN".to_string(), "ghp_secret".to_string()), ("EMPTY".to_string(), String::new())].into(),
                ..Default::default()
            }],
            ..Settings::default()
        }
    }

    #[test]
    fn secrets_leave_the_file_and_come_back() {
        let mut store = MapStore::default();
        let mut s = with_secrets();
        let used = protect(&mut s, &mut store, &BTreeSet::new());
        assert_eq!(used.len(), 2);
        let on_disk = serde_json::to_string(&s).unwrap();
        assert!(!on_disk.contains("Bearer abc") && !on_disk.contains("ghp_secret"), "{on_disk}");
        assert!(on_disk.contains("keyring:mcp:gh:env:TOKEN"));

        let mut loaded: Settings = serde_json::from_str(&on_disk).unwrap();
        let (found, missing) = reveal(&mut loaded, &store);
        assert!(missing.is_empty());
        assert_eq!(found, used);
        assert_eq!(loaded.webhooks[0].headers["Authorization"], "Bearer abc");
        assert_eq!(loaded.mcp_servers[0].env["TOKEN"], "ghp_secret");
        assert_eq!(loaded.mcp_servers[0].env["EMPTY"], "");
    }

    #[test]
    fn protecting_twice_is_stable_and_removed_secrets_are_deleted() {
        let mut store = MapStore::default();
        let mut s = with_secrets();
        let used = protect(&mut s, &mut store, &BTreeSet::new());
        let again = protect(&mut s, &mut store, &used);
        assert_eq!(again, used, "references stay in use and are not deleted");
        assert_eq!(store.0.len(), 2);
        // The user removed the connector: its credential must go (nothing in `used` any more).
        s.webhooks.clear();
        s.mcp_servers.clear();
        protect(&mut s, &mut store, &used);
        assert!(store.0.is_empty(), "{:?}", store.0);
    }

    #[test]
    fn a_missing_credential_becomes_empty_not_a_crash() {
        let mut s = with_secrets();
        s.webhooks[0].headers.insert("Authorization".into(), "keyring:hook:notify:header:Authorization".into());
        let (_, missing) = reveal(&mut s, &MapStore::default());
        assert_eq!(missing, ["hook:notify:header:Authorization"]);
        assert_eq!(s.webhooks[0].headers["Authorization"], "");
    }
}
