use super::*;
use crate::values::{self, Number, Value as Stored};
use crate::{Error, error::require, platform, read_fs};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub path: String,
    pub base: String,
    pub delivery_target: String,
    pub expected_revision: String,
}

impl Request {
    pub fn new(path: &Path, base: &str, target: &str, expected: &str) -> Result<Self> {
        let raw = path.as_os_str().as_encoded_bytes();
        require(
            !raw.is_empty() && raw.len() <= 8192 && !raw.contains(&0),
            "repository_invalid_path",
        )?;
        require(
            !path.components().any(|c| c == Component::ParentDir),
            "repository_parent_traversal",
        )?;
        require(oid(base, 40) || oid(base, 64), "repository_invalid_base")?;
        validate_target(target)?;
        require(
            expected == UNREGISTERED
                || expected
                    .strip_prefix("repository-sha256:")
                    .is_some_and(|s| oid(s, 64)),
            "repository_invalid_expected_revision",
        )?;
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        Ok(Self {
            path: normalized_absolute(&absolute)?,
            base: base.to_owned(),
            delivery_target: target.to_owned(),
            expected_revision: expected.to_owned(),
        })
    }
}

fn normalized_absolute(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or(Error("repository_non_unicode_path"))?;
    require(
        path.is_absolute()
            && !text.contains('\0')
            && !path.components().any(|c| c == Component::ParentDir),
        "repository_invalid_locator",
    )?;
    // POSIX normpath/abspath retain exactly two leading slashes, not three.
    let prefix = if text.starts_with("//") && !text.starts_with("///") {
        "//"
    } else {
        "/"
    };
    let parts = text
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>();
    Ok(format!("{prefix}{}", parts.join("/")))
}

pub(super) fn oid(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn validate_target(target: &str) -> Result<()> {
    require(
        target.chars().count() <= 1024
            && target.starts_with("refs/heads/")
            && !target
                .chars()
                .any(|c| c <= ' ' || c == '\u{7f}' || "~^:?*[\\".contains(c))
            && !target.contains("..")
            && !target.contains("@{")
            && target.split('/').all(|p| {
                !p.is_empty() && !p.starts_with('.') && !p.ends_with('.') && !p.ends_with(".lock")
            }),
        "repository_invalid_delivery_target",
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Identity {
    #[serde(serialize_with = "serialize_native_integer")]
    pub device: String,
    #[serde(serialize_with = "serialize_native_integer")]
    pub inode: String,
}
// Only freshly observed platform integers are serialized into new authority.
// Reads retain arbitrary nonnegative persisted integers without float coercion.
fn serialize_native_integer<S: serde::Serializer>(
    text: &str,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_u64(text.parse().map_err(serde::ser::Error::custom)?)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Filesystem {
    pub root: Identity,
    pub git_dir: Identity,
    pub common_dir: Identity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Frozen {
    pub canonical_root: String,
    pub git_dir: String,
    pub common_dir: String,
    pub filesystem: Filesystem,
    pub object_format: String,
    pub base_commit: String,
    pub base_tree: String,
    pub delivery_target: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Record {
    pub schema_version: u32,
    pub kind: String,
    pub registration_id: String,
    #[serde(flatten)]
    pub frozen: Frozen,
    pub registered_at: String,
    pub original_request: Request,
}
pub(super) struct Authority {
    pub record: Record,
    pub value: Stored,
    pub bytes: Vec<u8>,
}
impl Authority {
    pub fn decode(bytes: Vec<u8>) -> Result<Self> {
        require(bytes.len() <= MAX_RECORD, "repository_record_size_limit")?;
        let value = values::parse(&bytes, values::Profile::Authority)?;
        let fields = [
            "schema_version",
            "kind",
            "registration_id",
            "canonical_root",
            "git_dir",
            "common_dir",
            "filesystem",
            "object_format",
            "base_commit",
            "base_tree",
            "delivery_target",
            "registered_at",
            "original_request",
        ];
        closed(&value, &fields)?;
        require(
            field(&value, "schema_version")?.is_integer_one(),
            "repository_invalid_record",
        )?;
        let original = field(&value, "original_request")?;
        closed(
            original,
            &["path", "base", "delivery_target", "expected_revision"],
        )?;
        let filesystem = field(&value, "filesystem")?;
        closed(filesystem, &["root", "git_dir", "common_dir"])?;
        let record = Record {
            schema_version: 1,
            kind: text(&value, "kind")?,
            registration_id: text(&value, "registration_id")?,
            registered_at: text(&value, "registered_at")?,
            original_request: Request {
                path: text(original, "path")?,
                base: text(original, "base")?,
                delivery_target: text(original, "delivery_target")?,
                expected_revision: text(original, "expected_revision")?,
            },
            frozen: Frozen {
                canonical_root: text(&value, "canonical_root")?,
                git_dir: text(&value, "git_dir")?,
                common_dir: text(&value, "common_dir")?,
                filesystem: Filesystem {
                    root: stored_identity(field(filesystem, "root")?)?,
                    git_dir: stored_identity(field(filesystem, "git_dir")?)?,
                    common_dir: stored_identity(field(filesystem, "common_dir")?)?,
                },
                object_format: text(&value, "object_format")?,
                base_commit: text(&value, "base_commit")?,
                base_tree: text(&value, "base_tree")?,
                delivery_target: text(&value, "delivery_target")?,
            },
        };
        require(
            record.schema_version == 1 && record.kind == "repository-registration",
            "repository_invalid_record",
        )?;
        let id = record.registration_id.as_bytes();
        require(
            id.len() == 36
                && id.iter().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        *b == b'-'
                    } else {
                        b.is_ascii_digit() || (b'a'..=b'f').contains(b)
                    }
                }),
            "repository_invalid_uuid",
        )?;
        require(
            super::timestamp::valid(&record.registered_at),
            "repository_invalid_timestamp",
        )?;
        let request = &record.original_request;
        require(
            Request::new(
                Path::new(&request.path),
                &request.base,
                &request.delivery_target,
                &request.expected_revision,
            )? == *request
                && request.expected_revision == UNREGISTERED
                && request.base == record.frozen.base_commit
                && request.delivery_target == record.frozen.delivery_target,
            "repository_inconsistent_request",
        )?;
        let length = match record.frozen.object_format.as_str() {
            "sha1" => 40,
            "sha256" => 64,
            _ => return Err(Error("repository_invalid_object_format")),
        };
        require(
            oid(&record.frozen.base_commit, length) && oid(&record.frozen.base_tree, length),
            "repository_invalid_object_id",
        )?;
        for locator in [
            &record.frozen.canonical_root,
            &record.frozen.git_dir,
            &record.frozen.common_dir,
        ] {
            let path = Path::new(locator);
            require(
                path.is_absolute()
                    && !locator.contains('\0')
                    && !path.components().any(|c| c == Component::ParentDir)
                    && normalized_absolute(path)? == *locator,
                "repository_invalid_locator",
            )?;
        }
        Ok(Self {
            record,
            value,
            bytes,
        })
    }
    pub fn revision(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"coven.repository-registration:v1\0");
        digest.update(&self.bytes);
        format!("repository-sha256:{:x}", digest.finalize())
    }
}

fn field<'a>(value: &'a Stored, key: &str) -> Result<&'a Stored> {
    value
        .as_object()
        .and_then(|v| v.get(key))
        .ok_or(Error("repository_invalid_record"))
}
fn text(value: &Stored, key: &str) -> Result<String> {
    field(value, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or(Error("repository_invalid_record"))
}
fn closed(value: &Stored, fields: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or(Error("repository_invalid_record"))?;
    require(
        object.entries().len() == fields.len() && fields.iter().all(|f| object.get(f).is_some()),
        "repository_invalid_record",
    )
}
fn stored_identity(value: &Stored) -> Result<Identity> {
    closed(value, &["device", "inode"])?;
    let integer = |key| -> Result<String> {
        match field(value, key)? {
            Stored::Number(Number::Integer(n)) if !n.as_str().starts_with('-') => {
                Ok(n.as_str().to_owned())
            }
            _ => Err(Error("repository_invalid_filesystem_identity")),
        }
    };
    Ok(Identity {
        device: integer("device")?,
        inode: integer("inode")?,
    })
}

pub(super) fn workspace(path: &Path) -> Result<PathBuf> {
    let root = read_fs::workspace(path)?;
    require(
        std::fs::symlink_metadata(root.join("state"))?.is_dir(),
        "repository_invalid_workspace",
    )?;
    read_fs::managed(&root, Path::new(AUTHORITY), false)?;
    Ok(root)
}
pub(super) fn read(root: &Path) -> Result<Option<Authority>> {
    let path = read_fs::managed(root, Path::new(AUTHORITY), false)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(_) => Authority::decode(read_fs::read_regular(&path, MAX_RECORD)?).map(Some),
    }
}
pub(super) fn identity(path: &Path) -> Result<Identity> {
    let location = platform::workspace_location(path)?;
    Ok(Identity {
        device: location.device.to_string(),
        inode: location.inode.to_string(),
    })
}
