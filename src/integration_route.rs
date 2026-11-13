//! Installer-only route pins. These are never receiver addresses or native IDs.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

const PREFIX: &str = "coven-route-v1:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route {
    FromKitsune { state_root: Option<String> },
}

fn valid_root(root: &Path) -> bool {
    root.is_absolute()
        && root
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && root
            .to_str()
            .is_some_and(|s| !s.chars().any(char::is_control))
}

/// Encode explicit environment selection for an installer's existing `socket` field.
/// An omitted root is resolved by the reporter from Kit's configured report argv.
pub fn installed_pin(state_root: Option<&Path>) -> Result<String> {
    if state_root.is_some_and(|p| !valid_root(p)) {
        return Err(Error("integration_route_absolute_path_required"));
    }
    Ok(format!(
        "{PREFIX}{}",
        serde_json::to_string(&Route::FromKitsune {
            state_root: state_root.map(|p| p.to_str().unwrap().to_owned()),
        })?
    ))
}

pub(crate) fn decode(pin: &str) -> Result<Option<Route>> {
    let Some(encoded) = pin.strip_prefix(PREFIX) else {
        return Ok(None);
    };
    let route: Route =
        serde_json::from_str(encoded).map_err(|_| Error("integration_route_invalid_pin"))?;
    let Route::FromKitsune { state_root } = &route;
    if installed_pin(state_root.as_deref().map(Path::new))? != pin {
        return Err(Error("integration_route_invalid_pin"));
    }
    Ok(Some(route))
}

pub(crate) fn validate(pin: &str, version: u32, legacy: u32, route: &Option<Route>) -> Result<()> {
    let decoded = decode(pin)?;
    if decoded != *route || version != legacy + u32::from(decoded.is_some()) {
        return Err(Error("integration_route_invalid_receipt"));
    }
    if decoded.is_none() && !valid_root(Path::new(pin)) {
        return Err(Error("integration_route_absolute_path_required"));
    }
    Ok(())
}

pub(crate) fn argv(pin: &str) -> Result<Vec<String>> {
    Ok(match decode(pin)? {
        None => vec!["--socket".into(), pin.into()],
        Some(Route::FromKitsune { state_root }) => {
            let mut args = vec!["--from-kitsune".into()];
            if let Some(root) = state_root {
                args.extend(["--state-root".into(), root]);
            }
            args
        }
    })
}

pub(crate) fn shell(pin: &str) -> String {
    argv(pin)
        .expect("validated installer route")
        .iter()
        .map(|s| {
            if s == "--socket" || s == "--from-kitsune" || s == "--state-root" {
                s.clone()
            } else {
                format!("'{}'", s.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_canonical_versioned_data_not_native_identifiers() {
        for bad in [
            "coven-route-v1:{}",
            "coven-route-v1:{\"kind\":\"socket\",\"socket\":\"/receiver\"}",
            "coven-route-v1:{\"kind\":\"from_kitsune\",\"state_root\":\"relative\"}",
            "coven-route-v1:{\"kind\":\"from_kitsune\",\"state_root\":null,\"session_id\":\"secret\"}",
            "coven-route-v1:{\"kind\":\"from_kitsune\",\"state_root\":null,\"state_root\":null}",
        ] {
            assert!(decode(bad).is_err(), "{bad}");
        }
        let pin = installed_pin(None).unwrap();
        let route = decode(&pin).unwrap();
        assert!(validate(&pin, 2, 1, &route).is_ok());
        assert!(validate(&pin, 1, 1, &route).is_err());
        assert!(validate(&pin, 2, 1, &None).is_err());
        assert!(validate("/receiver", 2, 1, &route).is_err());
        assert!(validate("/receiver", 1, 1, &None).is_ok());
        assert_eq!(argv(&pin).unwrap(), ["--from-kitsune"]);
    }
}
