//! S2 Items (UI-15, OP-SALES-10, BR-SALES-16): list, card, save, the item picker. Runs on the small demo seed.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

fn demo() -> axum::Router {
    router(AppState::new(db::open_demo(None).expect("demo database")))
}

/// A router and a second handle on the same data, by way of a temp file.
fn demo_with_conn() -> (axum::Router, Connection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let path = db::database_path(tmp.path());
    let (conn, _) = db::open(&path, db::Seed::Demo).unwrap();
    let app = router(AppState::new(conn));
    let check = Connection::open(&path).unwrap();
    (app, check, tmp)
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    (s, b)
}

async fn htmx_get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app, Request::get(uri).header("HX-Request", "true").body(Body::empty()).unwrap()).await;
    (s, b)
}

fn form(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("HX-Request", "true")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn the_list_is_sorted_by_name_with_the_layout_columns() {
    let app = demo();
    let (status, html) = get(&app, "/items").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains(r#"href="/items" aria-current="page""#), "the sidebar link is live and current");
    for column in ["Name", "SKU", "Description", "Sales Price/Rate", "Active"] {
        assert!(html.contains(&format!("<th>{column}</th>")) || html.contains(&format!(">{column}</th>")), "{column}");
    }
    assert!(html.contains("1–6 of 6"));
    let workshop = html.find("Discovery workshop").unwrap();
    let design = html.find("Design services").unwrap();
    let retainer = html.find(">Retainer<").unwrap();
    assert!(design < workshop && workshop < retainer, "sorted by name (kontacts 345)");
    assert!(html.contains("$1,200.00"));
}

#[tokio::test]
async fn search_covers_name_sku_and_description_as_a_fragment() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/items?q=prd-print").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<html"), "a fragment for htmx");
    assert!(html.contains("Large-format prints") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get(&app, "/items?q=nothing-like-this").await;
    assert!(html.contains("No items match."));
    let (_, html) = htmx_get(&app, "/items?q=%3Cscript%3E").await;
    assert!(!html.contains("<script>"), "the query is escaped");
}

#[tokio::test]
async fn the_card_has_the_layout_fields_and_no_account_fields() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/items/new").await;
    assert_eq!(status, StatusCode::OK);
    for name in [
        "name",
        "sku",
        "description",
        "unit_price",
        "taxable",
        "item_type",
        "purchase_cost",
        "purchase_desc",
        "qty_on_hand",
        "track_qty_on_hand",
        "inv_start_date",
        "level",
        "active",
    ] {
        assert!(html.contains(&format!(r#"name="{name}""#)), "{name}");
    }
    assert!(!html.to_lowercase().contains("account"), "M2: no account fields (D-02)");
    for value in ["Inventory", "Service", "NonInventory"] {
        assert!(html.contains(&format!(r#"value="{value}""#)), "value list 4 offers {value}");
    }
    // Q7: a new item's Active box is checked.
    let active = html.split(r#"name="active""#).nth(1).unwrap().split('>').next().unwrap();
    assert!(active.contains("checked"), "{active}");

    let (status, html) = htmx_get(&app, "/items/i-svc-workshop/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"value="Discovery workshop""#) && html.contains(r#"value="1200.00""#));
    assert!(html.contains(r#"hx-put="/items/i-svc-workshop""#));
    assert_eq!(htmx_get(&app, "/items/nobody/edit").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn name_is_required() {
    let (app, check, _tmp) = demo_with_conn();
    let before: i64 = check.query_row("SELECT COUNT(*) FROM item", [], |r| r.get(0)).unwrap();
    let (status, _, html) = send(&app, form("POST", "/items", "name=++&unit_price=5&active=on")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("aria-invalid"), "the card comes back with the error next to the field");
    let after: i64 = check.query_row("SELECT COUNT(*) FROM item", [], |r| r.get(0)).unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn unit_price_is_cents_with_two_decimals_typed() {
    let (app, check, _tmp) = demo_with_conn();
    // M5: 9.999 is refused, not rounded.
    let (status, _, html) = send(&app, form("POST", "/items", "name=Too+fine&unit_price=9.999")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("9.999"), "the typed value stays in the form");
    let (status, _, _) = send(&app, form("POST", "/items", "name=Too+big&purchase_cost=abc")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, headers, _) = send(
        &app,
        form(
            "POST",
            "/items",
            "name=Plotter&sku=PLT&description=Plot&unit_price=%241%2C200.00&taxable=on&item_type=Inventory\
             &purchase_cost=800.5&purchase_desc=Bought&qty_on_hand=2.5&track_qty_on_hand=on\
             &inv_start_date=2026-01-15&level=3&active=on",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["HX-Redirect"], "/items");
    let row: (i64, i64, i64, String, f64, i64, i64, String, String) = check
        .query_row(
            "SELECT unit_price, purchase_cost, taxable, item_type, qty_on_hand, track_qty_on_hand, level,
                    inv_start_date, purchase_desc FROM item WHERE name = 'Plotter'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (120000, 80050, 1, "Inventory".into(), 2.5, 1, 3, "2026-01-15".into(), "Bought".into()),
        "$1,200.00 is 120000 cents"
    );
}

#[tokio::test]
async fn other_fields_are_validated() {
    let app = demo();
    for (body, field) in [
        ("name=x&item_type=Bundle", "item_type"),
        ("name=x&inv_start_date=2026-02-30", "inv_start_date"),
        ("name=x&qty_on_hand=lots", "qty_on_hand"),
        ("name=x&level=high", "level"),
    ] {
        let (status, _, html) = send(&app, form("POST", "/items", body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(html.contains(&format!(r#"id="err-{field}""#)), "{field}");
    }
}

#[tokio::test]
async fn the_active_value_list_column_follows_every_save() {
    let (app, check, _tmp) = demo_with_conn();
    let read = |id: &str| -> Option<String> {
        check.query_row("SELECT name_for_active_value_list FROM item WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    };
    // Field 35: If ( active ; name ).
    let (status, headers, _) = send(&app, form("POST", "/items", "name=Fresh&active=on")).await;
    assert_eq!(status, StatusCode::OK, "{headers:?}");
    let id: String = check.query_row("SELECT id FROM item WHERE name = 'Fresh'", [], |r| r.get(0)).unwrap();
    assert_eq!(read(&id).as_deref(), Some("Fresh"));
    let uri = format!("/items/{id}");
    assert_eq!(send(&app, form("PUT", &uri, "name=Renamed&active=on")).await.0, StatusCode::OK);
    assert_eq!(read(&id).as_deref(), Some("Renamed"));
    assert_eq!(send(&app, form("PUT", &uri, "name=Renamed")).await.0, StatusCode::OK, "unchecked: inactive");
    assert_eq!(read(&id), None);
    let active: i64 = check.query_row("SELECT active FROM item WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(active, 0);
}

#[tokio::test]
async fn saving_an_item_answers_its_row_and_a_toast() {
    let app = demo();
    let (status, headers, html) =
        send(&app, form("PUT", "/items/i-svc-hourly", "name=Revision+hours&sku=SVC-HOURLY&unit_price=99.00&active=on"))
            .await;
    assert_eq!(status, StatusCode::OK);
    let trigger = headers["HX-Trigger"].to_str().unwrap();
    assert!(trigger.contains("karbon-toast") && trigger.contains("karbon-close-dialog"), "{trigger}");
    assert!(html.contains(r#"id="item-i-svc-hourly""#) && html.contains(r#"hx-swap-oob="true""#));
    assert!(html.contains("Revision hours") && html.contains("$99.00"));
    assert_eq!(send(&app, form("PUT", "/items/nobody", "name=x")).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn inactive_item_hidden_from_picker_still_on_old_lines() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/pickers/item").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"value="i-svc-workshop""#), "offered while active");
    assert!(html.contains(r#"data-unit-price="1200.00""#), "the picker carries what a line copies (X4)");
    let (_, html) = htmx_get(&app, "/pickers/item?q=SVC-SURVEY").await;
    assert!(html.contains(r#"value="i-svc-survey""#) && !html.contains(r#"value="i-svc-workshop""#));

    let body = "name=Discovery+workshop&sku=SVC-WORKSHOP&description=Discovery+workshop&unit_price=1200.00";
    assert_eq!(send(&app, form("PUT", "/items/i-svc-workshop", body)).await.0, StatusCode::OK);
    let (_, html) = htmx_get(&app, "/pickers/item").await;
    assert!(!html.contains(r#"value="i-svc-workshop""#), "BR-SALES-16: not offered on new lines");
    // BR-SALES-16: it stays on the old lines and in the list.
    let (status, html) = get(&app, "/sales/INV-1048").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Discovery workshop"));
    let (_, html) = get(&app, "/items").await;
    assert!(html.contains("Discovery workshop"), "M4: still in the list, marked inactive");
    assert!(html.contains("Inactive"));
}

#[tokio::test]
async fn there_is_no_delete() {
    let app = demo();
    let req = Request::delete("/items/i-svc-hourly").body(Body::empty()).unwrap();
    let (status, _, _) = send(&app, req).await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
        "M4: no delete route, got {status}"
    );
    assert!(!karbon_server::ROUTES.iter().any(|(m, p)| *m == "DELETE" && p.starts_with("/items")));
}

#[tokio::test]
async fn an_unknown_field_is_refused() {
    let app = demo();
    let (status, _, _) = send(&app, form("POST", "/items", "name=x&qbo_id=7")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = send(&app, form("PUT", "/items/i-svc-hourly", "name=x&name_for_active_value_list=y")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_retainer_item_cannot_be_renamed_but_can_be_deactivated() {
    let (app, check, _tmp) = demo_with_conn();
    // Q4: BR-SALES-07 matches on the name.
    let (status, _, html) = send(&app, form("PUT", "/items/i-retainer", "name=Deposit&active=on")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains(r#"id="err-name""#));
    let name: String = check.query_row("SELECT name FROM item WHERE id = 'i-retainer'", [], |r| r.get(0)).unwrap();
    assert_eq!(name, "Retainer");
    let (status, _, _) = send(&app, form("PUT", "/items/i-retainer", "name=Retainer&sku=RETAINER")).await;
    assert_eq!(status, StatusCode::OK, "deactivating is allowed");
    let (name, active): (String, i64) = check
        .query_row("SELECT name, active FROM item WHERE id = 'i-retainer'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((name.as_str(), active), ("Retainer", 0));
    // Field-level: another item may still be saved under its own name.
    assert_eq!(send(&app, form("PUT", "/items/i-svc-hourly", "name=Hours&active=on")).await.0, StatusCode::OK);
}
