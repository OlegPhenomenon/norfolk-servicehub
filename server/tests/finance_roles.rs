//! Finance actions belong to one audience: only the applicant pays online.
mod support;
use serde_json::json;

#[tokio::test]
async fn only_the_applicant_can_open_an_online_checkout() {
    let (mut d, _dir) = support::fixture().await;
    let slot = d.hall("rawson-main", 21);
    let (c, _) = d.submit("alexey", "rawson-hall-hire", slot, None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    let money = d.money("olga", c).await.unwrap();
    assert_eq!(money["staff"], true);
    let invoice =
        money["invoices"].as_array().unwrap().iter().find(|i| i["kind"] == "invoice").expect("issued invoice")["id"]
            .clone();
    for staff in ["olga", "tom"] {
        let refused = d
            .expect(staff, "POST", &format!("/api/cases/{c}/checkout"), json!({"invoice_id":invoice}), 403)
            .await
            .unwrap();
        assert!(refused.to_string().contains("Only the applicant can pay online"), "{staff}: {refused}");
    }
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkout_sessions WHERE case_id=?")
        .bind(c)
        .fetch_one(&d.state.db)
        .await
        .unwrap();
    assert_eq!(sessions, 0, "a refused staff checkout opens no provider session");
    d.pay("alexey", c, false).await.unwrap();
    let money = d.money("alexey", c).await.unwrap();
    assert_eq!(money["summary"]["outstanding_cents"], 0);
}
