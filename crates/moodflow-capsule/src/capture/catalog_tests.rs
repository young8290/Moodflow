use super::{Asset, RETENTION_SECONDS, catalog::*, prune_for_space};
use serde_json::json;
use std::collections::BTreeSet;

fn asset(i: u8, format: &str, created_at: u64, bytes: u64) -> Asset {
    serde_json::from_value(json!({
        "id": format!("00000000-0000-4000-8000-{i:012}"),
        "extension": format, "created_at": created_at, "bytes": bytes, "width": 16, "height": 9,
        "duration_ms": 0, "sha256": "a".repeat(64)
    }))
    .unwrap()
}

#[test]
fn legacy_index_defaults_are_preserved_and_filters_do_not_change_totals() {
    let mut assets = [
        asset(1, "png", 2, 10),
        asset(2, "png", 2, 20),
        asset(3, "mp4", 1, 50),
        asset(4, "gif", 3, 17),
        asset(5, "png", 5, 25),
    ];
    assert_eq!(Scope::of(&assets[0]), Scope::Temporary);
    assert!(
        serde_json::to_value(&assets[0])
            .unwrap()
            .get("name")
            .is_none()
    );
    assets[0].name = "迭代 Review".into();
    assets[1].name = "迭代 REVIEW".into();
    assets[2].kept = true;
    assets[3].permanent = true;
    assets[4].trashed_at = Some(6);
    let leases = BTreeSet::from([assets[1].id.clone()]);
    let filters: Query = serde_json::from_value(json!({
        "search": "review 迭代 PNG", "format": "png", "limit": 1, "offset": 400
    }))
    .unwrap();
    let now = RETENTION_SECONDS + 10;
    let result = query(&assets, &leases, &filters, now).unwrap();
    assert_eq!(result.storage.total.bytes, 122);
    assert_eq!(result.storage.trash.bytes, 25);
    assert_eq!(result.storage.kept.bytes, 50);
    assert_eq!(result.storage.library.bytes, 17);
    assert_eq!(result.storage.expired.count, 1);
    assert_eq!(result.matched.count, 2);
    assert_eq!(result.matched.bytes, 30);
    assert_eq!(result.offset, 1);
    assert_eq!(result.rows[0].asset.id, assets[1].id);
    assert!(result.rows[0].in_use);

    let recent: Query = serde_json::from_value(json!({"days": 7})).unwrap();
    assert_eq!(
        query(&assets, &leases, &recent, now).unwrap().matched.count,
        0
    );
    let trash: Query = serde_json::from_value(json!({"scope":"trash", "days":7})).unwrap();
    assert_eq!(
        query(&assets, &leases, &trash, 10).unwrap().rows[0]
            .asset
            .id,
        assets[4].id
    );
    let largest: Query = serde_json::from_value(json!({"sort":"largest"})).unwrap();
    assert_eq!(
        query(&assets, &leases, &largest, now).unwrap().rows[0]
            .asset
            .id,
        assets[2].id
    );
    assert_eq!(
        prune_for_space(&assets, &leases, now, 0).unwrap(),
        [assets[0].id.clone()]
    );
}

#[test]
fn revision_scope_and_leases_are_independent_preconditions() {
    let mut asset = asset(1, "png", 0, 10);
    let mut item = Selection {
        id: asset.id.clone(),
        revision: revision(&asset),
    };
    assert_eq!(
        review(None, &item, Operation::Keep, false),
        Outcome::Missing
    );
    assert_eq!(
        review(Some(&asset), &item, Operation::Trash, true),
        Outcome::InUse
    );
    assert_eq!(
        review(Some(&asset), &item, Operation::Export, true),
        Outcome::Ready
    );
    assert_eq!(
        review(Some(&asset), &item, Operation::Purge, false),
        Outcome::WrongState
    );
    asset.name = "新名称".into();
    assert_eq!(
        review(Some(&asset), &item, Operation::Trash, false),
        Outcome::Stale
    );
    item.revision = revision(&asset);
    asset.permanent = true;
    assert_eq!(
        review(Some(&asset), &item, Operation::Trash, false),
        Outcome::Stale
    );
    item.revision = revision(&asset);
    assert_eq!(
        review(Some(&asset), &item, Operation::Trash, false),
        Outcome::WrongState
    );
    assert!(validate_batch(&[item.clone(), item.clone()]).is_err());
    item.id = "../asset.png".into();
    assert!(validate_batch(&[item]).is_err());
    assert!(normalize_name("my\nasset").is_err());
    assert!(normalize_name(&"中".repeat(81)).is_err());
}
