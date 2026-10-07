use crate::{error::AppResult, state::AppState, storage, time};
use sqlx::SqliteConnection;
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    for t in [
        "development_approval",
        "building_approval",
        "modification_approval",
        "planning_certificate",
        "complaint_response",
        "road_response",
        "service_response",
    ] {
        let label = super::decisions::label(t);
        let body = "{{decision_type_label}}\nCase {{case_number}} — {{service_name}}\nApplicant: {{applicant_name}}\nProperty (Portion/Lot): {{property_ref}}\nInformation as recorded on {{decision_date}}\nReasons: {{reasons}}\nConditions: {{conditions}}\nEvidence: {{evidence_list}}\nAuthorised by: {{approver_name}}";
        sqlx::query("INSERT INTO decision_templates(code,version,name,decision_type,body_template,active,created_at) VALUES(?,1,?,?,?,1,?) ON CONFLICT(code,version) DO NOTHING").bind(format!("demo_{t}")).bind(format!("Fictional {label} — version 1")).bind(t).bind(body).bind(time::now_str()).execute(&mut *tx).await?;
    }
    for name in ["site-plan.pdf", "elevation.pdf"] {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM documents_seed_files WHERE name=?)")
            .bind(name)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            let bytes = sample_pdf(name == "elevation.pdf");
            if std::env::var_os("SERVICEHUB_EXPORT_DOCUMENT_SAMPLES").is_some() {
                let folder = state.cfg.seed_data_dir.join("docs");
                std::fs::create_dir_all(&folder)?;
                std::fs::write(folder.join(name), &bytes)?;
            }
            let staged = storage::stage(state, &bytes, name, storage::AllowList::Docs).await?;
            let blob = storage::register(tx, staged, None).await?;
            sqlx::query("INSERT INTO documents_seed_files(name,blob_id) VALUES(?,?)")
                .bind(name)
                .bind(blob.id)
                .execute(&mut *tx)
                .await?;
        }
    }
    Ok(())
}

/// Small schematic plans with a clearly labelled private owner-detail area.
fn sample_pdf(elevation: bool) -> Vec<u8> {
    use printpdf::{BuiltinFont, Color, Line, Mm, PdfDocument, Point, Rgb};
    let title = if elevation { "Fictional elevation drawing A-201" } else { "Fictional Site plan A-101" };
    let (doc, page, layer) = PdfDocument::new(title, Mm(210.0), Mm(297.0), "Fictional drawing");
    let layer = doc.get_page(page).get_layer(layer);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).expect("builtin font");
    layer.use_text(crate::pdf::HEADER, 9.0, Mm(15.0), Mm(282.0), &font);
    layer.use_text(title, 16.0, Mm(15.0), Mm(263.0), &font);
    layer.use_text("Portion DEMO-44, fictional Taylors Road site", 10.0, Mm(15.0), Mm(251.0), &font);
    if elevation {
        layer.use_text("Owner details: CONFIDENTIAL-OWNER-PHONE 0412 345 678", 10.0, Mm(15.0), Mm(237.0), &font);
    }
    layer.set_outline_color(Color::Rgb(Rgb::new(0.15, 0.2, 0.2, None)));
    layer.set_outline_thickness(0.8);
    let draw = |points: &[(f32, f32)], closed| {
        layer.add_line(Line {
            points: points.iter().map(|(x, y)| (Point::new(Mm(*x), Mm(*y)), false)).collect(),
            is_closed: closed,
        });
    };
    if elevation {
        draw(&[(35.0, 100.0), (175.0, 100.0), (175.0, 170.0), (35.0, 170.0)], true);
        draw(&[(25.0, 170.0), (105.0, 215.0), (185.0, 170.0)], true);
        draw(&[(60.0, 130.0), (90.0, 130.0), (90.0, 150.0), (60.0, 150.0)], true);
        draw(&[(120.0, 100.0), (145.0, 100.0), (145.0, 150.0), (120.0, 150.0)], true);
        draw(&[(20.0, 98.0), (190.0, 98.0)], false);
        layer.use_text("North elevation - illustrative only", 11.0, Mm(45.0), Mm(82.0), &font);
    } else {
        draw(&[(30.0, 75.0), (180.0, 75.0), (180.0, 220.0), (30.0, 220.0)], true);
        draw(&[(65.0, 120.0), (145.0, 120.0), (145.0, 185.0), (65.0, 185.0)], true);
        draw(&[(145.0, 145.0), (172.0, 145.0), (172.0, 75.0)], false);
        draw(&[(30.0, 65.0), (180.0, 65.0)], false);
        draw(&[(43.0, 193.0), (43.0, 211.0), (40.0, 205.0), (43.0, 211.0), (46.0, 205.0)], false);
        layer.use_text("N", 10.0, Mm(40.0), Mm(215.0), &font);
        layer.use_text("Dwelling footprint", 10.0, Mm(78.0), Mm(150.0), &font);
        layer.use_text("Access", 9.0, Mm(150.0), Mm(108.0), &font);
        layer.use_text("Fictional Taylors Road frontage", 10.0, Mm(48.0), Mm(52.0), &font);
    }
    layer.use_text(
        "Schematic only - not surveyed, not to scale, not for construction.",
        9.0,
        Mm(15.0),
        Mm(30.0),
        &font,
    );
    layer.use_text(crate::pdf::FOOTER, 7.0, Mm(15.0), Mm(15.0), &font);
    doc.save_to_bytes().expect("sample PDF serialisation")
}
