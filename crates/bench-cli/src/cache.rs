use crate::{
    models::CacheInfo,
    util::{ensure_dir, sha256_bytes},
};
use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum CacheLookup<T> {
    Hit(T),
    Miss(CacheInfo),
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct CacheIndexEntry {
    key: String,
    fingerprint: Value,
}

pub fn key_for(fingerprint: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(fingerprint)?;
    Ok(sha256_bytes(&bytes))
}

pub fn logical_id(parts: &[&str]) -> String {
    sha256_bytes(parts.join("\0").as_bytes())
}

pub fn lookup<T>(
    root: &Path,
    namespace: &str,
    logical_id: &str,
    key: &str,
    fingerprint: &Value,
) -> Result<CacheLookup<T>>
where
    T: DeserializeOwned,
{
    let index = read_index(root, namespace, logical_id);
    let mut invalidated_by = match &index {
        Ok(Some(entry)) if entry.key != key => fingerprint_diff(&entry.fingerprint, fingerprint),
        Err(_) => vec!["cache_index_unreadable".to_string()],
        _ => Vec::new(),
    };

    if invalidated_by.is_empty() {
        let path = value_path(root, namespace, key);
        if path.exists() {
            match fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))
                .and_then(|text| serde_json::from_str::<T>(&text).context("parsing cache entry"))
            {
                Ok(value) => return Ok(CacheLookup::Hit(value)),
                Err(_) => invalidated_by.push("cache_entry_unreadable".to_string()),
            }
        } else if matches!(index, Ok(Some(ref entry)) if entry.key == key) {
            invalidated_by.push("cache_entry_missing".to_string());
        }
    }

    let info = if invalidated_by.is_empty() {
        CacheInfo::miss(key)
    } else {
        CacheInfo::stale(key, invalidated_by)
    };
    Ok(CacheLookup::Miss(info))
}

pub fn store<T>(
    root: &Path,
    namespace: &str,
    logical_id: &str,
    key: &str,
    fingerprint: &Value,
    value: &T,
) -> Result<()>
where
    T: Serialize,
{
    let value_path = value_path(root, namespace, key);
    if let Some(parent) = value_path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(&value_path, serde_json::to_string_pretty(value)?)
        .with_context(|| format!("writing {}", value_path.display()))?;

    let index_path = index_path(root, namespace, logical_id);
    if let Some(parent) = index_path.parent() {
        ensure_dir(parent)?;
    }
    let index = CacheIndexEntry {
        key: key.to_string(),
        fingerprint: fingerprint.clone(),
    };
    fs::write(&index_path, serde_json::to_string_pretty(&index)?)
        .with_context(|| format!("writing {}", index_path.display()))?;
    Ok(())
}

fn read_index(root: &Path, namespace: &str, logical_id: &str) -> Result<Option<CacheIndexEntry>> {
    let path = index_path(root, namespace, logical_id);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("parsing {}", path.display()))
}

fn value_path(root: &Path, namespace: &str, key: &str) -> PathBuf {
    root.join(".cache")
        .join("bench-cli")
        .join(namespace)
        .join("values")
        .join(format!("{key}.json"))
}

fn index_path(root: &Path, namespace: &str, logical_id: &str) -> PathBuf {
    root.join(".cache")
        .join("bench-cli")
        .join(namespace)
        .join("index")
        .join(format!("{logical_id}.json"))
}

fn fingerprint_diff(old: &Value, new: &Value) -> Vec<String> {
    let mut paths = BTreeSet::new();
    collect_diff_paths("", old, new, &mut paths);
    let mut paths: Vec<_> = paths.into_iter().collect();
    if paths.is_empty() && old != new {
        paths.push("fingerprint".to_string());
    }
    paths.truncate(16);
    paths
}

fn collect_diff_paths(path: &str, old: &Value, new: &Value, paths: &mut BTreeSet<String>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Value::Object(left), Value::Object(right)) => {
            let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
            for key in keys {
                let next = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };
                match (left.get(key), right.get(key)) {
                    (Some(left), Some(right)) => collect_diff_paths(&next, left, right, paths),
                    _ => {
                        paths.insert(next);
                    }
                }
            }
        }
        _ => {
            paths.insert(path.to_string());
        }
    }
}
