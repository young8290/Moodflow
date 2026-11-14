//! Capture protocol, pixel limits, scroll overlap and temporary-asset retention. No OS APIs.
pub mod catalog;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_PIXELS: u64 = 64_000_000;
pub const MAX_EDGE: u32 = 32768;
pub const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Compare biased uptime (includes sleep) with active interrupt time; wall-clock changes are irrelevant.
#[derive(Default)]
pub struct SleepGuard {
    baseline: Option<i128>,
    revoked: bool,
}
impl SleepGuard {
    pub fn observe(&mut self, uptime_ms: u64, active_100ns: u64) -> bool {
        let bias = i128::from(uptime_ms) - i128::from(active_100ns / 10000);
        let baseline = *self.baseline.get_or_insert(bias);
        if bias - baseline > 200 {
            self.revoked = true;
        }
        !self.revoked
    }
}

pub fn validate_size(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0
            && height > 0
            && width <= MAX_EDGE
            && height <= MAX_EDGE
            && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "capture.pixel_limit"
    );
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Crop {
    pub fn validate(&self, width: u32, height: u32) -> Result<()> {
        validate_size(self.width, self.height)?;
        ensure!(
            self.x.checked_add(self.width).is_some_and(|v| v <= width)
                && self.y.checked_add(self.height).is_some_and(|v| v <= height),
            "capture.invalid_crop"
        );
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Monitor { index: usize },
    Window { hwnd: u64, pid: u32 },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: String,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub crop: Option<Crop>,
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default)]
    pub seconds: u32,
    #[serde(default)]
    pub cursor: bool,
    #[serde(default)]
    pub audio: bool,
    #[serde(default)]
    pub gif: bool,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [
                "enumerate",
                "screenshot",
                "record",
                "scroll",
                "ocr",
                "normalize",
                "qr",
                "inspect",
                "dib",
                "inspect_target"
            ]
            .contains(&self.operation.as_str()),
            "capture.invalid_operation"
        );
        ensure!(
            self.delay_ms <= 10000 && self.seconds <= 120,
            "capture.time_limit"
        );
        ensure!(
            self.output.as_ref().is_none_or(|p| p.len() <= 32768)
                && self.input.as_ref().is_none_or(|p| p.len() <= 32768),
            "capture.invalid_path"
        );
        if self.operation == "record" {
            ensure!(self.seconds > 0, "capture.time_limit");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub extension: String,
    pub created_at: u64,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    #[serde(default)]
    pub permanent: bool,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporary_since: Option<u64>,
}
impl Asset {
    pub fn retention_start(&self) -> u64 {
        self.temporary_since.unwrap_or(self.created_at)
    }
    pub fn validate(&self) -> Result<()> {
        super::canonical_id(&self.id)?;
        validate_size(self.width, self.height)?;
        ensure!(
            catalog::normalize_name(&self.name)? == self.name
                && !(self.permanent && self.kept)
                && (self.trashed_at.is_none() || !(self.permanent || self.kept)),
            "capture.invalid_asset"
        );
        ensure!(
            ["png", "mp4", "gif"].contains(&self.extension.as_str())
                && self.bytes > 0
                && self.bytes <= MAX_ASSET_BYTES
                && self.duration_ms <= 120_000
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
            "capture.invalid_asset"
        );
        Ok(())
    }
}
/// Expired or oldest-over-budget temporary assets only; callers hold leases until I/O completes.
pub fn prune_candidates(
    assets: &[Asset],
    leased: &BTreeSet<String>,
    now: u64,
) -> Result<Vec<String>> {
    prune_for_space(assets, leased, now, 0)
}
pub fn prune_for_space(
    assets: &[Asset],
    leased: &BTreeSet<String>,
    now: u64,
    incoming: u64,
) -> Result<Vec<String>> {
    ensure!(incoming <= MAX_ASSET_BYTES, "capture.file_limit");
    ensure!(assets.len() <= 10000, "capture.asset_limit");
    let mut seen = BTreeSet::new();
    let mut bytes = 0_u64;
    for asset in assets {
        asset.validate()?;
        ensure!(seen.insert(&asset.id), "capture.duplicate_asset");
        bytes = bytes
            .checked_add(asset.bytes)
            .ok_or_else(|| anyhow::anyhow!("capture.asset_limit"))?;
    }
    let mut eligible = assets
        .iter()
        .filter(|a| catalog::Scope::of(a) == catalog::Scope::Temporary && !leased.contains(&a.id))
        .collect::<Vec<_>>();
    eligible.sort_by_key(|a| (a.created_at, &a.id));
    let mut removed = Vec::new();
    for asset in eligible {
        if now.saturating_sub(asset.retention_start()) >= RETENTION_SECONDS
            || bytes.saturating_add(incoming) > MAX_TOTAL_BYTES
        {
            bytes -= asset.bytes;
            removed.push(asset.id.clone());
        }
    }
    Ok(removed)
}

/// Determine a reliable overlap for two equally sized RGBA screenshots. Never invent missing rows.
pub fn vertical_overlap(
    previous: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Result<Option<u32>> {
    validate_size(width, height)?;
    let expected = width as usize * height as usize * 4;
    ensure!(
        previous.len() == expected && next.len() == expected,
        "capture.invalid_pixels"
    );
    let stride = width as usize * 4;
    if previous == next {
        return Ok(Some(height));
    }
    for overlap in (32..height).rev() {
        let mut mismatch = 0;
        let mut samples = 0;
        for row in 0..16_u32 {
            let y = row * (overlap - 1) / 15;
            for column in 0..32_u32 {
                let x = column * (width - 1) / 31;
                let a = (height - overlap + y) as usize * stride + x as usize * 4;
                let b = y as usize * stride + x as usize * 4;
                samples += 1;
                if (0..3).any(|c| previous[a + c].abs_diff(next[b + c]) > 6) {
                    mismatch += 1;
                }
            }
        }
        if mismatch == 0 && samples > 0 {
            let a = &previous[(height - overlap) as usize * stride..];
            let b = &next[..overlap as usize * stride];
            if a == b {
                return Ok(Some(overlap));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixels_crops_and_scroll_overlap_are_bounded_and_exact_for_fixture_rows() {
        assert!(validate_size(8000, 8000).is_ok());
        assert!(validate_size(8001, 8000).is_err());
        assert!(
            Crop {
                x: u32::MAX,
                y: 0,
                width: 10,
                height: 10
            }
            .validate(100, 100)
            .is_err()
        );
        let frame = |start: u32| {
            (0..100)
                .flat_map(|y| {
                    (0..64).flat_map(move |x| {
                        let value = ((start + y) * 79 + x * 17) as u8;
                        [value, value.wrapping_add(31), value.wrapping_add(71), 255]
                    })
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            vertical_overlap(&frame(0), &frame(40), 64, 100).unwrap(),
            Some(60)
        );
        assert_eq!(
            vertical_overlap(&frame(0), &frame(0), 64, 100).unwrap(),
            Some(100)
        );
        assert!(vertical_overlap(&[], &frame(0), 64, 100).is_err());
    }
    #[test]
    fn active_and_permanent_assets_survive_expiry_and_pressure() {
        let asset = |permanent| Asset {
            id: uuid::Uuid::new_v4().to_string(),
            extension: "png".into(),
            created_at: 1,
            bytes: 100,
            width: 10,
            height: 10,
            duration_ms: 0,
            permanent,
            sha256: "a".repeat(64),
            name: String::new(),
            kept: false,
            trashed_at: None,
            temporary_since: None,
        };
        let assets = vec![asset(false), asset(false), asset(true)];
        let leased = BTreeSet::from([assets[1].id.clone()]);
        assert_eq!(
            prune_candidates(&assets, &leased, RETENTION_SECONDS + 1).unwrap(),
            vec![assets[0].id.clone()]
        );
        let mut pressure = (1..=20)
            .map(|created_at| {
                let mut item = asset(false);
                item.created_at = created_at;
                item.bytes = MAX_ASSET_BYTES;
                item
            })
            .collect::<Vec<_>>();
        pressure[1].permanent = true;
        let protected = BTreeSet::from([pressure[0].id.clone()]);
        assert_eq!(
            prune_for_space(&pressure, &protected, 21, MAX_ASSET_BYTES).unwrap(),
            [pressure[2].id.clone()]
        );
    }
}
#[test]
fn sleep_revokes_the_task_permanently_but_active_work_does_not() {
    let mut guard = SleepGuard::default();
    assert!(guard.observe(1000, 1000 * 10000));
    assert!(guard.observe(11000, 11000 * 10000));
    assert!(!guard.observe(21000, 12000 * 10000));
    assert!(!guard.observe(22000, 22000 * 10000));
}

#[cfg(test)]
mod catalog_tests;
