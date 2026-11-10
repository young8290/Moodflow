//! Signed catalogs stay in the core-owned plugin directory. The Hub only fetches bytes;
//! source pin, signature, time, revision, package hash and consent are rechecked here.
use crate::plugins;
use anyhow::{Result, anyhow, bail};
use moodflow_market::{catalog, inspect_package};
use moodflow_plugin_api::{Platform, Version};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().ok_or_else(|| anyhow!("market.invalid"))
}
fn current(root: &Path, now: i64) -> Result<Value> {
    let policy = catalog::load(root)?;
    let revision = catalog::revision(&policy);
    let Some(policy) = policy else {
        return Ok(
            json!({"source":null,"revision":revision,"items":[],"revoked":[],"expired":false}),
        );
    };
    let index = policy.verified(now, false)?;
    let installed = moodflow_market::installed(root);
    let items: Vec<Value> = index.as_ref().map(|index| {
        index.plugins.iter().map(|item| {
            let release = index.latest_compatible(&item.id, moodflow_coordinator::PLUGIN_HOST_VERSION, Platform::Windows);
            let from = installed.iter().find(|(id,_)| id == &item.id).map(|(_,v)| v);
            let update = from.zip(release).is_some_and(|(from,to)| Version::parse(&to.version) > Version::parse(from));
            json!({"id":item.id,"name":item.name,"author":item.author,"description":item.description,
                "repo":item.repo,"release":release,"installed":from,"update":update})
        }).collect()
    }).unwrap_or_default();
    let revoked: Vec<Value> = index
        .as_ref()
        .map(|index| {
            installed
                .iter()
                .filter_map(|(id, version)| {
                    index
                        .revoked
                        .iter()
                        .find(|r| r.id == *id && r.version.as_ref().is_none_or(|v| v == version))
                        .map(|r| json!({"id":id,"version":version,"reason":r.reason}))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(
        json!({"source":policy.source,"revision":revision,"items":items,"revoked":revoked,
        "fingerprint":policy.source.as_ref().map(|s| s.fingerprint()).transpose()?,
        "generatedAt":index.as_ref().map(|v| &v.generated_at),
        "expired":index.as_ref().is_some_and(|i| catalog::check_fresh(i,now).is_err())}),
    )
}
fn plan(
    root: &Path,
    params: &Value,
    now: i64,
) -> Result<(moodflow_market::InstallPlan, Option<String>)> {
    let policy = catalog::load(root)?.ok_or_else(|| anyhow!("market.not_configured"))?;
    if policy.revision() != string(params, "revision")? {
        bail!("market.source_changed");
    }
    let index = policy
        .verified(now, true)?
        .ok_or_else(|| anyhow!("market.not_configured"))?;
    let id = string(params, "id")?;
    let version = string(params, "version")?;
    if index.is_revoked(id, version) {
        bail!("market.withdrawn");
    }
    let release = index
        .latest_compatible(
            id,
            moodflow_coordinator::PLUGIN_HOST_VERSION,
            Platform::Windows,
        )
        .filter(|r| r.version == version)
        .ok_or_else(|| anyhow!("market.incompatible"))?;
    if params.get("sha256").is_some() && string(params, "sha256")? != release.sha256 {
        bail!("market.hash");
    }
    let path = Path::new(string(params, "path")?);
    let meta = std::fs::symlink_metadata(path).map_err(|_| anyhow!("market.io"))?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > moodflow_market::MAX_PACKAGE_BYTES
    {
        bail!("market.too_large");
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| anyhow!("market.io"))?
        .take(moodflow_market::MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("market.io"))?;
    if format!("{:x}", Sha256::digest(&bytes)) != release.sha256.to_ascii_lowercase() {
        bail!("market.hash");
    }
    let from = moodflow_market::installed(root)
        .into_iter()
        .find(|(i, _)| i == id)
        .map(|(_, v)| v);
    if from
        .as_ref()
        .is_some_and(|v| Version::parse(v) >= Version::parse(version))
    {
        bail!("market.already_installed");
    }
    let old = from
        .as_ref()
        .and_then(|v| moodflow_market::installed_manifest(root, id, v));
    let plan = inspect_package(
        &bytes,
        id,
        release,
        moodflow_coordinator::PLUGIN_HOST_VERSION,
        old.as_ref(),
    )
    .map_err(|_| anyhow!("market.package"))?;
    Ok((plan, from))
}
/// Returns whether loaded plugins need to be replaced immediately.
pub(crate) fn rpc(method: &str, params: &Value, root: &Path, now: i64) -> Result<(Value, bool)> {
    match method {
        "plugins.market.catalog" => Ok((current(root, now)?, false)),
        "plugins.market.configure" => {
            if params["confirmed"].as_bool() != Some(true) {
                bail!("market.consent");
            }
            let source = if params["source"].is_null() {
                None
            } else {
                Some(
                    serde_json::from_value(params["source"].clone())
                        .map_err(|_| anyhow!("market.invalid"))?,
                )
            };
            catalog::configure(root, string(params, "revision")?, source)?;
            Ok((current(root, now)?, true))
        }
        "plugins.market.accept" => {
            catalog::accept(
                root,
                string(params, "revision")?,
                string(params, "index")?.into(),
                string(params, "signature")?.into(),
                now,
            )?;
            Ok((current(root, now)?, true))
        }
        "plugins.market.inspect" => {
            let (plan, from) = plan(root, params, now)?;
            let mut preview = plugins::preview_json(&plan, from.as_deref());
            preview["source"] = json!("signed");
            preview["sha256"] = params["sha256"].clone();
            Ok((preview, false))
        }
        "plugins.market.install" => {
            let _policy_lock = catalog::acquire(root)?;
            // Required here even for clients which omitted it at preview.
            string(params, "sha256")?;
            let (plan, _) = plan(root, params, now)?;
            let accept: Vec<String> = serde_json::from_value(params["accept"].clone())
                .map_err(|_| anyhow!("market.consent"))?;
            plugins::check_consent(&plan, &accept)?;
            moodflow_market::install(&plan, root).map_err(|_| anyhow!("market.io"))?;
            Ok((
                json!({"id":plan.manifest.id,"version":plan.manifest.version}),
                true,
            ))
        }
        _ => bail!("market.invalid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::io::Write;
    const NOW: i64 = 1794225600;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn package() -> Vec<u8> {
        let text = r#"id="org.example.demo"
name="Demo"
version="1.0.0"
sdk=1
min_host_version="0.1.0"
type="wasm"
entry="plugin.wasm"
platforms=["windows"]
license="MIT"
[contributes]
candidate_provider={}
[capabilities]
storage=true
"#;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("manifest.toml", text.as_bytes()),
            ("plugin.wasm", b"\0asm\x01\0\0\0".as_slice()),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn real_package_consent_hash_revision_install_and_offline_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("plugins");
        let key = SigningKey::from_bytes(&[15; 32]);
        let bytes = package();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let (cfg,_)=rpc("plugins.market.configure",&json!({"revision":"","confirmed":true,"source":{
            "label":"Fixture","url":"https://example.com/index.json","publicKey":hex(key.verifying_key().as_bytes())}}),&root,NOW).unwrap();
        let mut index = json!({"version":1,"generated_at":chrono::DateTime::from_timestamp(NOW,0).unwrap().to_rfc3339(),
            "plugins":[{"id":"org.example.demo","name":"Demo","author":"Test","repo":"https://example.com/source","description":"Demo",
            "releases":[{"version":"1.0.0","url":"https://example.com/demo.mfpack","sha256":hash,"size":bytes.len(),"min_host_version":"0.1.0","platforms":["windows"]}]}],"revoked":[]});
        let text = index.to_string();
        let (view,_)=rpc("plugins.market.accept",&json!({"revision":cfg["revision"],"index":text,"signature":hex(&key.sign(text.as_bytes()).to_bytes())}),&root,NOW).unwrap();
        let file = dir.path().join("download.mfpack");
        std::fs::write(&file, &bytes).unwrap();
        let mut args = json!({"path":file,"id":"org.example.demo","version":"1.0.0","revision":view["revision"],"sha256":hash,"accept":[]});
        let (preview, _) = rpc("plugins.market.inspect", &args, &root, NOW).unwrap();
        assert_eq!(preview["added"], json!(["storage"]));
        assert!(rpc("plugins.market.install", &args, &root, NOW).is_err());
        assert!(moodflow_market::installed(&root).is_empty());
        args["accept"] = json!(["storage"]);
        std::fs::write(&file, b"replacement").unwrap();
        assert!(rpc("plugins.market.install", &args, &root, NOW).is_err());
        std::fs::write(&file, &bytes).unwrap();
        index["generated_at"] = json!(
            chrono::DateTime::from_timestamp(NOW + 1, 0)
                .unwrap()
                .to_rfc3339()
        );
        let text = index.to_string();
        let (updated,_)=rpc("plugins.market.accept",&json!({"revision":view["revision"],"index":text,"signature":hex(&key.sign(text.as_bytes()).to_bytes())}),&root,NOW).unwrap();
        assert!(rpc("plugins.market.install", &args, &root, NOW).is_err());
        args["revision"] = updated["revision"].clone();
        assert!(
            rpc(
                "plugins.market.install",
                &args,
                &root,
                NOW + catalog::MAX_AGE_SECONDS + 2
            )
            .is_err()
        );
        rpc("plugins.market.install", &args, &root, NOW).unwrap();
        assert_eq!(
            moodflow_market::installed(&root),
            vec![("org.example.demo".into(), "1.0.0".into())]
        );
        index["generated_at"] = json!(
            chrono::DateTime::from_timestamp(NOW + 2, 0)
                .unwrap()
                .to_rfc3339()
        );
        index["revoked"] =
            json!([{"id":"org.example.demo","version":"1.0.0","reason":"withdrawn"}]);
        let text = index.to_string();
        rpc("plugins.market.accept",&json!({"revision":updated["revision"],"index":text,"signature":hex(&key.sign(text.as_bytes()).to_bytes())}),&root,NOW).unwrap();
        let list = plugins::list_json(Some(&root), &["org.example.demo".into()], false, &[]);
        assert_eq!(list["plugins"][0]["state"], "revoked");
        let view = current(&root, NOW + catalog::MAX_AGE_SECONDS + 3).unwrap();
        assert_eq!(view["expired"], true);
        assert_eq!(view["revoked"][0]["id"], "org.example.demo");
        assert!(view["items"][0]["release"].is_null());
        std::fs::write(root.join(catalog::POLICY_FILE), b"corrupt").unwrap();
        assert_eq!(
            plugins::list_json(Some(&root), &[], false, &[])["plugins"][0]["state"],
            "market_error"
        );
    }
}
