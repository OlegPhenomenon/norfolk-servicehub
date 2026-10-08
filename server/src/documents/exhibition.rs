//! Public files are independently rasterised copies. No public query exposes source IDs.
use crate::{
    auth::Actor,
    authz::Role,
    cases::core::Visibility,
    db::write_tx,
    error::{AppError, AppResult},
    state::AppState,
    storage,
    web::{ClientIp, Json, Path},
};
use axum::{
    Router,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use printpdf::image_crate as image;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::{
    path::{Path as FsPath, PathBuf},
    time::Duration,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Exhibition {
    pub id: i64,
    pub case_id: i64,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub opens_at: Option<String>,
    pub closes_at: Option<String>,
    pub prepared_by: i64,
    pub approved_by: Option<i64>,
    pub created_at: String,
    /// Formal early termination (status stays `closed`; projections report `terminated`).
    pub terminated_at: Option<String>,
    pub termination_reason: Option<String>,
    pub consideration_summary: Option<String>,
}
#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct Item {
    pub id: i64,
    pub exhibition_id: i64,
    pub source_document_version_id: i64,
    pub title: String,
    pub redactions_json: String,
    pub published_blob_id: Option<i64>,
}
#[derive(Deserialize)]
pub struct Input {
    pub case_id: i64,
    pub title: String,
    pub summary: String,
    pub opens_at: String,
    pub closes_at: String,
    pub expected_revision: i64,
}
#[derive(Deserialize)]
pub struct ItemInput {
    pub source_document_version_id: i64,
    pub title: String,
    #[serde(default)]
    pub redactions: Vec<Rect>,
    pub expected_revision: i64,
}
#[derive(Deserialize)]
pub struct Revision {
    pub expected_revision: i64,
}
#[derive(Deserialize)]
pub struct Submission {
    pub name: String,
    pub email: String,
    pub body: String,
}
#[derive(Deserialize)]
pub struct ReasonInput {
    #[serde(default)]
    pub reason: String,
    pub expected_revision: i64,
}
#[derive(Deserialize)]
pub struct ConsiderInput {
    #[serde(default)]
    pub outcome: String,
    pub expected_revision: i64,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/exhibitions", get(list).post(create))
        .route("/api/exhibitions/{id}", get(detail).put(edit))
        .route("/api/exhibitions/{id}/items", post(add_item))
        .route("/api/exhibitions/{id}/items/{item}", put(edit_item))
        .route("/api/exhibitions/{id}/items/{item}/pages/{page}", get(preview))
        .route("/api/exhibitions/{id}/withdraw", post(withdraw))
        .route("/api/exhibitions/lookup/{number}", get(lookup))
        .route("/api/exhibitions/{id}/items/{item}/redacted-pages/{page}", get(redacted_preview))
        .route("/api/exhibitions/{id}/publish", post(publish))
        .route("/api/exhibitions/{id}/submissions", get(submissions))
        .route("/api/exhibitions/{id}/submissions/{sid}/consider", post(consider))
        .route("/api/exhibitions/{id}/terminate", post(terminate))
        .route("/api/exhibitions/{id}/consideration", post(consideration_summary))
        .route("/api/cases/{id}/exhibition-not-required", post(not_required))
        .route("/api/public/exhibitions", get(public_list))
        .route("/api/public/exhibitions/{id}", get(public_detail))
        .route("/api/public/exhibitions/{id}/items/{item}/file", get(public_file))
        .route("/api/public/exhibitions/{id}/items/{item}/pages/{page}", get(public_preview))
        .route("/api/public/exhibitions/{id}/submissions", post(submit))
}
async fn load(tx: &mut SqliteConnection, id: i64) -> AppResult<Exhibition> {
    sqlx::query_as("SELECT * FROM exhibitions WHERE id=?")
        .bind(id)
        .fetch_optional(tx)
        .await?
        .ok_or_else(AppError::not_found)
}
async fn items(tx: &mut SqliteConnection, id: i64) -> AppResult<Vec<Item>> {
    Ok(sqlx::query_as("SELECT * FROM exhibition_items WHERE exhibition_id=? ORDER BY id")
        .bind(id)
        .fetch_all(tx)
        .await?)
}
async fn staff(tx: &mut SqliteConnection, actor: &Actor, id: i64) -> AppResult<Exhibition> {
    let e = load(tx, id).await?;
    super::manage(tx, actor, e.case_id, &[Role::Specialist, Role::Manager]).await?;
    Ok(e)
}
fn draft(e: &Exhibition) -> AppResult<()> {
    if e.status != "draft" {
        return Err(AppError::conflict("Published exhibitions are immutable. Prepare a new exhibition for changes."));
    }
    Ok(())
}
fn window(input: &Input) -> AppResult<(String, String)> {
    super::text("title", &input.title, 200)?;
    super::text("summary", &input.summary, 10000)?;
    let opens = crate::time::parse(&input.opens_at)
        .map_err(|_| AppError::field("opens_at", "Enter a date and time with a timezone."))?;
    let closes = crate::time::parse(&input.closes_at)
        .map_err(|_| AppError::field("closes_at", "Enter a date and time with a timezone."))?;
    if closes <= opens {
        return Err(AppError::field("closes_at", "Closing time must be after opening time."));
    }
    Ok((opens.to_rfc3339(), closes.to_rfc3339()))
}
pub fn validate_rectangles(rects: &[Rect]) -> AppResult<()> {
    if rects.len() > 300 {
        return Err(AppError::field("redactions", "Use at most 300 rectangles."));
    }
    for r in rects {
        if r.page == 0
            || r.page > 30
            || ![r.x, r.y, r.w, r.h].iter().all(|n| n.is_finite())
            || r.x < 0.0
            || r.y < 0.0
            || r.w <= 0.0
            || r.h <= 0.0
            || r.x + r.w > 1.0
            || r.y + r.h > 1.0
        {
            return Err(AppError::field(
                "redactions",
                "Rectangles must fit within pages 1–30, with coordinates between 0 and 1.",
            ));
        }
    }
    Ok(())
}
pub async fn close_due(tx: &mut SqliteConnection, now: &str) -> AppResult<()> {
    let rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT id,case_id FROM exhibitions WHERE status='open' AND julianday(closes_at)<=julianday(?)")
            .bind(now)
            .fetch_all(&mut *tx)
            .await?;
    for (id, cid) in rows {
        sqlx::query("UPDATE exhibitions SET status='closed' WHERE id=? AND status='open'")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        super::changed(
            tx,
            None,
            cid,
            "documents.exhibition_closed",
            Visibility::Applicant,
            "The public exhibition comment window has closed.",
        )
        .await?;
    }
    Ok(())
}
async fn close_read(state: &AppState) -> AppResult<()> {
    let mut tx = write_tx(&state.db).await?;
    close_due(&mut tx, &state.now().to_rfc3339()).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn list(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Vec<Value>>> {
    actor.require_any_role(&[Role::Specialist, Role::Manager])?;
    close_read(&state).await?;
    let mut c = state.db.acquire().await?;
    let rows: Vec<Exhibition> = sqlx::query_as("SELECT * FROM exhibitions ORDER BY id DESC").fetch_all(&mut *c).await?;
    let mut out = vec![];
    for e in rows {
        if super::manage(&mut c, &actor, e.case_id, &[Role::Specialist, Role::Manager]).await.is_ok() {
            out.push(serde_json::to_value(e).map_err(|e| AppError::internal(e.to_string()))?);
        }
    }
    Ok(Json(out))
}
pub async fn create(State(state): State<AppState>, actor: Actor, Json(input): Json<Input>) -> AppResult<Json<Value>> {
    let (opens, closes) = window(&input)?;
    let mut tx = write_tx(&state.db).await?;
    let case = super::manage(&mut tx, &actor, input.case_id, &[Role::Specialist, Role::Manager]).await?;
    if case.module != "building" || case.is_confidential() {
        return Err(AppError::field(
            "case_id",
            "Public planning exhibitions require a non-confidential building case.",
        ));
    }
    crate::cases::core::bump_revision(&mut tx, case.id, Some(input.expected_revision)).await?;
    let id:i64=sqlx::query_scalar("INSERT INTO exhibitions(case_id,title,summary,status,opens_at,closes_at,prepared_by,created_at) VALUES(?,?,?,'draft',?,?,?,?) RETURNING id").bind(case.id).bind(input.title).bind(input.summary).bind(opens).bind(closes).bind(actor.user_id).bind(state.now().to_rfc3339()).fetch_one(&mut *tx).await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        case.id,
        "documents.exhibition_prepare",
        Visibility::Staff,
        "Prepared a draft public exhibition; a second staff member must approve publication.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
pub async fn edit(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    let (opens, closes) = window(&input)?;
    let mut tx = write_tx(&state.db).await?;
    let e = staff(&mut tx, &actor, id).await?;
    draft(&e)?;
    if e.case_id != input.case_id {
        return Err(AppError::field("case_id", "An exhibition cannot be moved to another case."));
    }
    crate::cases::core::bump_revision(&mut tx, e.case_id, Some(input.expected_revision)).await?;
    sqlx::query("UPDATE exhibitions SET title=?,summary=?,opens_at=?,closes_at=?,prepared_by=? WHERE id=?")
        .bind(input.title)
        .bind(input.summary)
        .bind(opens)
        .bind(closes)
        .bind(actor.user_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        e.case_id,
        "documents.exhibition_edit",
        Visibility::Staff,
        "Updated the draft public notice and comment window.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
pub async fn detail(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    close_read(&state).await?;
    let mut c = state.db.acquire().await?;
    let e = staff(&mut c, &actor, id).await?;
    let revision = crate::cases::core::load_case(&mut c, e.case_id).await?.revision;
    Ok(Json(json!({"exhibition":e,"items":items(&mut c,id).await?,"revision":revision})))
}
async fn save_item(state: &AppState, actor: &Actor, id: i64, item: Option<i64>, input: ItemInput) -> AppResult<i64> {
    super::text("title", &input.title, 200)?;
    validate_rectangles(&input.redactions)?;
    let mut tx = write_tx(&state.db).await?;
    let e = staff(&mut tx, actor, id).await?;
    draft(&e)?;
    let (case, _, _, _, _) = super::uploads::version_access(&mut tx, actor, input.source_document_version_id).await?;
    if case.id != e.case_id {
        return Err(AppError::field("source_document_version_id", "Choose a version from this case."));
    }
    crate::cases::core::bump_revision(&mut tx, e.case_id, Some(input.expected_revision)).await?;
    let redactions = serde_json::to_string(&input.redactions).map_err(|e| AppError::internal(e.to_string()))?;
    let item_id = if let Some(item) = item {
        let result=sqlx::query("UPDATE exhibition_items SET source_document_version_id=?,title=?,redactions_json=? WHERE id=? AND exhibition_id=?").bind(input.source_document_version_id).bind(input.title).bind(redactions).bind(item).bind(id).execute(&mut *tx).await?;
        if result.rows_affected() != 1 {
            return Err(AppError::not_found());
        }
        item
    } else {
        sqlx::query_scalar("INSERT INTO exhibition_items(exhibition_id,source_document_version_id,title,redactions_json,created_at) VALUES(?,?,?,?,?) RETURNING id").bind(id).bind(input.source_document_version_id).bind(input.title).bind(redactions).bind(state.now().to_rfc3339()).fetch_one(&mut *tx).await?
    };
    sqlx::query("UPDATE exhibitions SET prepared_by=? WHERE id=?")
        .bind(actor.user_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        e.case_id,
        "documents.exhibition_item",
        Visibility::Staff,
        "Updated an exhibition item and its redaction rectangles.",
    )
    .await?;
    tx.commit().await?;
    Ok(item_id)
}
pub async fn add_item(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<ItemInput>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({"id":save_item(&state,&actor,id,None,input).await?})))
}
pub async fn edit_item(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, item)): Path<(i64, i64)>,
    Json(input): Json<ItemInput>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!({"id":save_item(&state,&actor,id,Some(item),input).await?})))
}
struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> AppResult<Self> {
        let p = std::env::temp_dir().join(format!("servicehub-exhibition-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&p)?;
        Ok(Self(p))
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn command(name: &str, args: &[String]) -> AppResult<std::process::Output> {
    let mut c = tokio::process::Command::new(name);
    c.args(args).kill_on_drop(true);
    #[cfg(unix)]
    unsafe {
        c.pre_exec(|| {
            let mut budgets = vec![(libc::RLIMIT_FSIZE, 64 * 1024 * 1024), (libc::RLIMIT_CPU, 30)];
            // macOS rejects memory rlimits; dimensions/pixels and the semaphore apply everywhere.
            #[cfg(target_os = "linux")]
            budgets.push((libc::RLIMIT_AS, 768 * 1024 * 1024));
            for (resource, bytes) in budgets.drain(..) {
                let limit = libc::rlimit { rlim_cur: bytes, rlim_max: bytes };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let output = tokio::time::timeout(Duration::from_secs(30), c.output())
        .await
        .map_err(|_| AppError::field("file", "Page rendering exceeded 30 seconds."))?
        .map_err(|e| AppError::field("file", format!("{name} is required to render exhibition PDFs: {e}")))?;
    if !output.status.success() {
        return Err(AppError::field("file", "The file could not be rendered safely."));
    }
    Ok(output)
}
async fn render_pages(source: &FsPath, dir: &FsPath, preview: Option<u32>) -> AppResult<Vec<PathBuf>> {
    let info = command("pdfinfo", &[source.display().to_string()]).await?;
    let count = String::from_utf8_lossy(&info.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Pages:").and_then(|s| s.trim().parse::<u32>().ok()))
        .ok_or_else(|| AppError::field("file", "Could not read the PDF page count."))?;
    if count == 0 || count > 30 {
        return Err(AppError::field("file", "Exhibition PDFs must have 1–30 pages."));
    }
    if preview.is_some_and(|p| p == 0 || p > count) {
        return Err(AppError::not_found());
    }
    let mut total_pixels = 0u64;
    let mut total_bytes = 0u64;
    for page in preview.map(|p| p..=p).unwrap_or(1..=count) {
        let args = vec![
            "-scale-to".into(),
            "1800".into(),
            "-png".into(),
            "-singlefile".into(),
            "-f".into(),
            page.to_string(),
            "-l".into(),
            page.to_string(),
            source.display().to_string(),
            dir.join(format!("page-{page}")).display().to_string(),
        ];
        command("pdftoppm", &args).await?;
        let output = dir.join(format!("page-{page}.png"));
        let (width, height) =
            image::image_dimensions(&output).map_err(|_| AppError::field("file", "Invalid rendered page."))?;
        total_pixels += u64::from(width) * u64::from(height);
        total_bytes += std::fs::metadata(&output)?.len();
        if width > 1800 || height > 1800 || total_pixels > 40_000_000 || total_bytes > 40 * 1024 * 1024 {
            return Err(AppError::field("file", "PDF exceeds the rendering budget."));
        }
    }
    let mut pages: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    pages.sort_by_key(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit('-').next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    });
    if pages.len() != preview.map(|_| 1).unwrap_or(count as usize) {
        return Err(AppError::field("file", "Not every source page was rendered."));
    }
    Ok(pages)
}
fn burn(img: &mut image::RgbImage, rects: &[Rect], page: u32) {
    let (width, height) = img.dimensions();
    for r in rects.iter().filter(|r| r.page == page) {
        let x = (r.x * f64::from(width)).floor() as u32;
        let y = (r.y * f64::from(height)).floor() as u32;
        let right = ((r.x + r.w) * f64::from(width)).ceil().min(f64::from(width)) as u32;
        let bottom = ((r.y + r.h) * f64::from(height)).ceil().min(f64::from(height)) as u32;
        for yy in y..bottom {
            for xx in x..right {
                img.put_pixel(xx, yy, image::Rgb([0, 0, 0]));
            }
        }
    }
}
fn images_pdf(pages: Vec<PathBuf>, rects: &[Rect]) -> AppResult<Vec<u8>> {
    use printpdf::{ColorBits, ColorSpace, Image, ImageTransform, ImageXObject, Mm, PdfDocument, Px};
    let first =
        decode(&std::fs::read(pages.first().ok_or_else(|| AppError::field("file", "No pages were rendered."))?)?)?;
    let (doc, p, l) = PdfDocument::new(
        "",
        Mm(first.width() as f32 * 25.4 / 110.0),
        Mm(first.height() as f32 * 25.4 / 110.0),
        "Raster",
    );
    drop(first);
    let mut encoded_bytes = 0usize;
    for (i, path) in pages.into_iter().enumerate() {
        let mut img = decode(&std::fs::read(&path)?)?;
        burn(&mut img, rects, i as u32 + 1);
        let (w, h) = img.dimensions();
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 95)
            .encode_image(&img)
            .map_err(|_| AppError::field("file", "Could not encode the rendered page."))?;
        drop(img);
        std::fs::remove_file(path)?;
        encoded_bytes += encoded.len();
        if encoded_bytes > storage::MAX_BYTES {
            return Err(AppError::field("file", "Public PDF exceeds the output budget."));
        }
        let (p, l) = if i == 0 {
            (p, l)
        } else {
            doc.add_page(Mm(w as f32 * 25.4 / 110.0), Mm(h as f32 * 25.4 / 110.0), "Raster")
        };
        Image::from(ImageXObject {
            width: Px(w as usize),
            height: Px(h as usize),
            color_space: ColorSpace::Rgb,
            bits_per_component: ColorBits::Bit8,
            interpolate: false,
            image_data: encoded,
            image_filter: Some(printpdf::ImageFilter::DCT),
            clipping_bbox: None,
            smask: None,
        })
        .add_to_layer(doc.get_page(p).get_layer(l), ImageTransform { dpi: Some(110.0), ..Default::default() });
    }
    let bytes = doc.save_to_bytes().map_err(|e| AppError::internal(format!("Raster PDF: {e}")))?;
    let mut clean = printpdf::lopdf::Document::load_mem(&bytes).map_err(|e| AppError::internal(e.to_string()))?;
    clean.trailer.remove(b"Info");
    clean.trailer.remove(b"ID");
    for object in clean.objects.values_mut() {
        if let Ok(dict) = object.as_dict_mut() {
            dict.remove(b"Metadata");
            dict.remove(b"PieceInfo");
        }
    }
    clean.prune_objects();
    let mut out = vec![];
    clean.save_to(&mut out).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(out)
}
pub(crate) async fn redacted(source: &[u8], mime: &str, rects: &[Rect]) -> AppResult<(Vec<u8>, &'static str)> {
    let _permit = RENDERS.acquire().await.map_err(|_| AppError::internal("Rendering stopped"))?;
    validate_rectangles(rects)?;
    let dir = WorkDir::new()?;
    if mime == "application/pdf" {
        let src = dir.0.join("source.pdf");
        tokio::fs::write(&src, source).await?;
        let pages = render_pages(&src, &dir.0, None).await?;
        if rects.iter().any(|r| r.page as usize > pages.len()) {
            return Err(AppError::field("redactions", "A rectangle refers to a page that does not exist."));
        }
        let rects = rects.to_vec();
        let bytes = tokio::task::spawn_blocking(move || images_pdf(pages, &rects))
            .await
            .map_err(|e| AppError::internal(e.to_string()))??;
        Ok((bytes, "public-copy.pdf"))
    } else {
        if rects.iter().any(|r| r.page != 1) {
            return Err(AppError::field("redactions", "Image files have one page."));
        }
        let mut img = decode(source)?;
        burn(&mut img, rects, 1);
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageOutputFormat::Png)
            .map_err(|_| AppError::field("file", "Could not encode the public image."))?;
        Ok((out.into_inner(), "public-copy.png"))
    }
}
fn decode(bytes: &[u8]) -> AppResult<image::RgbImage> {
    let mut reader = image::io::Reader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| AppError::field("file", "Could not decode the image."))?;
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(10000);
    limits.max_image_height = Some(10000);
    limits.max_alloc = Some(200 * 1024 * 1024);
    reader.limits(limits);
    let decoded =
        reader.decode().map_err(|_| AppError::field("file", "The image cannot be safely decoded."))?.to_rgba8();
    let mut rgb = image::RgbImage::new(decoded.width(), decoded.height());
    for (x, y, p) in decoded.enumerate_pixels() {
        let alpha = u32::from(p[3]);
        let mut color = [0u8; 3];
        for c in 0..3 {
            color[c] = ((u32::from(p[c]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
        }
        rgb.put_pixel(x, y, image::Rgb(color));
    }
    Ok(rgb)
}
pub async fn preview(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, item, page)): Path<(i64, i64, String)>,
) -> AppResult<Response> {
    let page: u32 = page.strip_suffix(".png").and_then(|s| s.parse().ok()).ok_or_else(AppError::not_found)?;
    let mut c = state.db.acquire().await?;
    staff(&mut c, &actor, id).await?;
    let i = items(&mut c, id).await?.into_iter().find(|i| i.id == item).ok_or_else(AppError::not_found)?;
    let (_, _, _, blob, _) = super::uploads::version_access(&mut c, &actor, i.source_document_version_id).await?;
    drop(c);
    render_preview(&state, blob, page).await
}
async fn render_preview(state: &AppState, blob: i64, page: u32) -> AppResult<Response> {
    let (row, bytes) = storage::read(state, blob).await?;
    render_bytes(&bytes, &row.mime, page).await
}
async fn render_bytes(bytes: &[u8], mime: &str, page: u32) -> AppResult<Response> {
    let _permit = RENDERS.acquire().await.map_err(|_| AppError::internal("Rendering stopped"))?;
    let png = if mime == "application/pdf" {
        let dir = WorkDir::new()?;
        let src = dir.0.join("source.pdf");
        tokio::fs::write(&src, bytes).await?;
        let pages = render_pages(&src, &dir.0, Some(page)).await?;
        tokio::fs::read(&pages[0]).await?
    } else {
        if page != 1 {
            return Err(AppError::not_found());
        }
        let img = decode(bytes)?;
        let mut out = std::io::Cursor::new(vec![]);
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageOutputFormat::Png)
            .map_err(|e| AppError::internal(e.to_string()))?;
        out.into_inner()
    };
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "private, no-store")], png).into_response())
}
pub async fn publish(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Revision>,
) -> AppResult<Json<Value>> {
    let mut c = state.db.acquire().await?;
    let e = staff(&mut c, &actor, id).await?;
    draft(&e)?;
    if e.prepared_by == actor.user_id {
        return Err(AppError::forbidden_msg("A second staff member must approve publication."));
    }
    let case = crate::cases::core::load_case(&mut c, e.case_id).await?;
    if case.revision != input.expected_revision {
        return Err(AppError::stale_revision());
    }
    if e.closes_at.as_deref().is_none_or(|s| crate::time::parse(s).is_ok_and(|d| d <= state.now())) {
        return Err(AppError::field("closes_at", "Choose a closing time in the future."));
    }
    let snapshot = items(&mut c, id).await?;
    if snapshot.is_empty() {
        return Err(AppError::field("items", "Add at least one publication item."));
    }
    let mut sources = vec![];
    for i in &snapshot {
        let (_, _, _, blob, _) = super::uploads::version_access(&mut c, &actor, i.source_document_version_id).await?;
        sources.push(blob);
    }
    drop(c);
    let mut staged = vec![];
    for (i, blob) in snapshot.iter().zip(sources) {
        let (b, bytes) = storage::read(&state, blob).await?;
        let rects: Vec<Rect> = serde_json::from_str(&i.redactions_json)
            .map_err(|_| AppError::field("redactions", "Invalid rectangles."))?;
        let (copy, name) = redacted(&bytes, &b.mime, &rects).await?;
        staged.push(storage::stage(&state, &copy, name, storage::AllowList::Docs).await?);
    }
    let mut tx = write_tx(&state.db).await?;
    let current = staff(&mut tx, &actor, id).await?;
    draft(&current)?;
    if current.prepared_by == actor.user_id {
        return Err(AppError::forbidden_msg("A second staff member must approve publication."));
    }
    if current.closes_at.as_deref().and_then(|s| crate::time::parse(s).ok()).is_none_or(|d| d <= state.now()) {
        return Err(AppError::field(
            "closes_at",
            "The comment window closed during rendering. Update the window before publishing.",
        ));
    }
    crate::cases::core::bump_revision(&mut tx, current.case_id, Some(input.expected_revision)).await?;
    for (i, staged) in snapshot.into_iter().zip(staged) {
        super::uploads::version_access(&mut tx, &actor, i.source_document_version_id).await?;
        let b = storage::register(&mut tx, staged, actor.db_id()).await?;
        sqlx::query("UPDATE exhibition_items SET published_blob_id=? WHERE id=? AND exhibition_id=?")
            .bind(b.id)
            .bind(i.id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE exhibitions SET status='open',approved_by=? WHERE id=?")
        .bind(actor.user_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    super::changed(&mut tx,actor.db_id(),current.case_id,"documents.exhibition_publish",Visibility::Applicant,"Public exhibition published after review by a second staff member; only independently redacted copies are public.").await?;
    let case = crate::cases::core::load_case(&mut tx, current.case_id).await?;
    super::notify_applicant(
        &mut tx,
        &case,
        "Your planning proposal is on public exhibition",
        "The comment window and redacted documents are available on Public notices.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
fn public(e: &Exhibition, now: chrono::DateTime<chrono::Utc>) -> bool {
    matches!(e.status.as_str(), "open" | "closed")
        && e.opens_at.as_deref().and_then(|s| crate::time::parse(s).ok()).is_some_and(|d| d <= now)
        && e.closes_at
            .as_deref()
            .and_then(|s| crate::time::parse(s).ok())
            .is_some_and(|d| d > now - chrono::Duration::days(90))
}
fn public_projection(e: &Exhibition) -> Value {
    json!({"id":e.id,"title":e.title,"summary":e.summary,"status":display_status(e),"opens_at":e.opens_at,"closes_at":e.closes_at,"terminated_at":e.terminated_at,"termination_reason":e.termination_reason})
}
pub async fn public_list(State(state): State<AppState>) -> AppResult<Json<Vec<Value>>> {
    close_read(&state).await?;
    let rows: Vec<Exhibition> =
        sqlx::query_as("SELECT * FROM exhibitions WHERE status IN ('open','closed') ORDER BY closes_at DESC")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(rows.iter().filter(|e| public(e, state.now())).map(public_projection).collect()))
}
pub async fn public_detail(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    close_read(&state).await?;
    let mut c = state.db.acquire().await?;
    let e = load(&mut c, id).await?;
    if !public(&e, state.now()) {
        return Err(AppError::not_found());
    }
    let out=items(&mut c,id).await?.iter().filter(|i|i.published_blob_id.is_some()).map(|i|json!({"id":i.id,"title":i.title,"file_url":format!("/api/public/exhibitions/{id}/items/{}/file",i.id),"preview_url":format!("/api/public/exhibitions/{id}/items/{}/pages/1.png",i.id)})).collect::<Vec<_>>();
    Ok(Json(json!({"exhibition":public_projection(&e),"items":out})))
}
pub async fn public_file(State(state): State<AppState>, Path((id, item)): Path<(i64, i64)>) -> AppResult<Response> {
    let blob = public_blob(&state, id, item).await?;
    super::uploads::blob_response(&state, blob, false).await
}
pub async fn public_preview(
    State(state): State<AppState>,
    Path((id, item, page)): Path<(i64, i64, String)>,
) -> AppResult<Response> {
    let page: u32 = page.strip_suffix(".png").and_then(|s| s.parse().ok()).ok_or_else(AppError::not_found)?;
    let blob = public_blob(&state, id, item).await?;
    render_preview(&state, blob, page).await
}
async fn public_blob(state: &AppState, id: i64, item: i64) -> AppResult<i64> {
    close_read(state).await?;
    let mut c = state.db.acquire().await?;
    let e = load(&mut c, id).await?;
    if !public(&e, state.now()) {
        return Err(AppError::not_found());
    }
    let blob: Option<i64> =
        sqlx::query_scalar("SELECT published_blob_id FROM exhibition_items WHERE id=? AND exhibition_id=?")
            .bind(item)
            .bind(id)
            .fetch_optional(&mut *c)
            .await?
            .flatten();
    drop(c);
    blob.ok_or_else(AppError::not_found)
}
fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}
pub async fn submit(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(id): Path<i64>,
    Json(input): Json<Submission>,
) -> AppResult<Json<Value>> {
    if !state.rate.check(&format!("exhibition-submit:{ip}")) {
        return Err(AppError::rate_limited());
    }
    super::text("name", &input.name, 120)?;
    super::text("body", &input.body, 10000)?;
    super::text("email", &input.email, 254)?;
    if !valid_email(&input.email) {
        return Err(AppError::field("email", "Enter a valid email address."));
    }
    let mut tx = write_tx(&state.db).await?;
    close_due(&mut tx, &state.now().to_rfc3339()).await?;
    let e = load(&mut tx, id).await?;
    if !public(&e, state.now()) || e.status != "open" {
        return Err(AppError::conflict("The public comment window is closed."));
    }
    let sid:i64=sqlx::query_scalar("INSERT INTO public_submissions(exhibition_id,name,email,body,status,created_at) VALUES(?,?,?,?,'received',?) RETURNING id").bind(id).bind(input.name).bind(input.email).bind(input.body).bind(state.now().to_rfc3339()).fetch_one(&mut *tx).await?;
    super::changed(
        &mut tx,
        None,
        e.case_id,
        "documents.public_submission",
        Visibility::Staff,
        "A public exhibition submission was received.",
    )
    .await?;
    crate::notify::send(
        &mut tx,
        crate::notify::Notice {
            user_id: Some(e.prepared_by),
            email: None,
            phone: None,
            case_id: Some(e.case_id),
            subject: "Public submission needs consideration".into(),
            body: "Review the new submission in the exhibition editor.".into(),
            link: Some(format!("/staff/exhibitions/{id}")),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":sid})))
}
pub async fn submissions(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<Value>>> {
    let mut c = state.db.acquire().await?;
    staff(&mut c, &actor, id).await?;
    let rows = sqlx::query(
        "SELECT s.id,s.name,s.email,s.body,s.status,s.created_at,s.outcome,s.considered_at,u.display_name considered_by FROM public_submissions s LEFT JOIN users u ON u.id=s.considered_by WHERE s.exhibition_id=? ORDER BY s.id",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    Ok(Json(rows.iter().map(|r|json!({"id":r.get::<i64,_>("id"),"name":r.get::<String,_>("name"),"email":r.get::<String,_>("email"),"body":r.get::<String,_>("body"),"status":r.get::<String,_>("status"),"created_at":r.get::<String,_>("created_at"),"outcome":r.get::<Option<String>,_>("outcome"),"considered_at":r.get::<Option<String>,_>("considered_at"),"considered_by":r.get::<Option<String>,_>("considered_by")})).collect()))
}
pub async fn consider(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, sid)): Path<(i64, i64)>,
    Json(input): Json<ConsiderInput>,
) -> AppResult<Json<Value>> {
    super::text("outcome", &input.outcome, 5000)?;
    let mut tx = write_tx(&state.db).await?;
    let e = staff(&mut tx, &actor, id).await?;
    crate::cases::core::bump_revision(&mut tx, e.case_id, Some(input.expected_revision)).await?;
    let r = sqlx::query("UPDATE public_submissions SET status='considered',outcome=?,considered_by=?,considered_at=? WHERE id=? AND exhibition_id=? AND status='received'")
        .bind(input.outcome.trim())
        .bind(actor.user_id)
        .bind(state.now().to_rfc3339())
        .bind(sid)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() != 1 {
        return Err(AppError::conflict("This submission already has a recorded consideration outcome."));
    }
    super::changed(
        &mut tx,
        actor.db_id(),
        e.case_id,
        "documents.submission_considered",
        Visibility::Staff,
        &format!("Public submission {sid} considered. Outcome: {}", input.outcome.trim()),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":sid})))
}
/// Formal early termination of an open exhibition. Comments already received still need consideration.
async fn terminate(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<ReasonInput>,
) -> AppResult<Json<Value>> {
    super::text("reason", &input.reason, 5000)?;
    let mut tx = write_tx(&state.db).await?;
    close_due(&mut tx, &state.now().to_rfc3339()).await?;
    let e = staff(&mut tx, &actor, id).await?;
    if e.status != "open" {
        return Err(AppError::conflict("Only an open exhibition can be terminated early."));
    }
    crate::cases::core::bump_revision(&mut tx, e.case_id, Some(input.expected_revision)).await?;
    sqlx::query("UPDATE exhibitions SET status='closed',terminated_at=?,terminated_by=?,termination_reason=? WHERE id=? AND status='open'")
        .bind(state.now().to_rfc3339())
        .bind(actor.user_id)
        .bind(input.reason.trim())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        e.case_id,
        "documents.exhibition_terminated",
        Visibility::Applicant,
        &format!("The public exhibition was formally terminated early. Reason: {}", input.reason.trim()),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
/// One recorded consideration covering every submission still without an individual outcome.
async fn consideration_summary(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<ReasonInput>,
) -> AppResult<Json<Value>> {
    super::text("reason", &input.reason, 20000)?;
    let mut tx = write_tx(&state.db).await?;
    close_due(&mut tx, &state.now().to_rfc3339()).await?;
    let e = staff(&mut tx, &actor, id).await?;
    if e.status != "closed" {
        return Err(AppError::conflict("Record the consideration summary after the comment window has closed."));
    }
    crate::cases::core::bump_revision(&mut tx, e.case_id, Some(input.expected_revision)).await?;
    let now = state.now().to_rfc3339();
    sqlx::query("UPDATE exhibitions SET consideration_summary=?,considered_by=?,considered_at=? WHERE id=?")
        .bind(input.reason.trim())
        .bind(actor.user_id)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let covered = sqlx::query("UPDATE public_submissions SET status='considered',outcome=?,considered_by=?,considered_at=? WHERE exhibition_id=? AND status='received'")
        .bind(format!("Covered by the consideration summary: {}", input.reason.trim()))
        .bind(actor.user_id)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    super::changed(
        &mut tx,
        actor.db_id(),
        e.case_id,
        "documents.exhibition_considered",
        Visibility::Applicant,
        &format!("Consideration of public submissions recorded ({covered} covered by the summary)."),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"covered":covered})))
}
/// "Exhibition not required for this case", with its mandatory reason (history + visible to the applicant).
async fn not_required(
    State(state): State<AppState>,
    actor: Actor,
    Path(case_id): Path<i64>,
    Json(input): Json<ReasonInput>,
) -> AppResult<Json<Value>> {
    super::text("reason", &input.reason, 5000)?;
    let mut tx = write_tx(&state.db).await?;
    let case = super::manage(&mut tx, &actor, case_id, &[Role::Specialist, Role::Manager]).await?;
    if case.module != "building" || !crate::cases::workflow::is_open(&case) {
        return Err(AppError::conflict("Only an open building request has a public exhibition stage."));
    }
    close_due(&mut tx, &state.now().to_rfc3339()).await?;
    let open: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM exhibitions WHERE case_id=? AND status='open')")
        .bind(case_id)
        .fetch_one(&mut *tx)
        .await?;
    if open {
        return Err(AppError::conflict(
            "A public exhibition is open on this request. Formally terminate it with a reason instead.",
        ));
    }
    crate::cases::core::bump_revision(&mut tx, case_id, Some(input.expected_revision)).await?;
    record_not_required(&mut tx, actor.db_id(), case_id, input.reason.trim(), &state.now().to_rfc3339()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn record_not_required(
    tx: &mut SqliteConnection,
    actor: Option<i64>,
    case_id: i64,
    reason: &str,
    now: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO exhibition_not_required(case_id,reason,decided_by,decided_at) VALUES(?,?,?,?)")
        .bind(case_id)
        .bind(reason)
        .bind(actor)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    super::changed(
        tx,
        actor,
        case_id,
        "documents.exhibition_not_required",
        Visibility::Applicant,
        &format!("Public exhibition is not required for this request. Reason: {reason}"),
    )
    .await
}
/// Blocks while an exhibition on the case is open or any public submission lacks a consideration outcome.
pub async fn case_block(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Option<String>> {
    close_due(tx, &crate::time::now_str()).await?;
    let open: Option<String> = sqlx::query_scalar(
        "SELECT closes_at FROM exhibitions WHERE case_id=? AND status='open' ORDER BY closes_at DESC LIMIT 1",
    )
    .bind(case_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(closes) = open {
        return Ok(Some(format!(
            "A public exhibition on this request is open until {closes}. Wait for it to close, or formally terminate it with a reason."
        )));
    }
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_submissions s JOIN exhibitions e ON e.id=s.exhibition_id WHERE e.case_id=? AND s.status='received'")
        .bind(case_id)
        .fetch_one(&mut *tx)
        .await?;
    Ok((pending > 0).then(|| {
        format!("{pending} public submission(s) still need a recorded consideration outcome (per comment or a consideration summary).")
    }))
}
/// Guard of the exhibition step: closed (or formally terminated) exhibition with every comment considered, or
/// a recorded "not required" decision.
pub async fn step_block(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Option<String>> {
    if let Some(block) = case_block(tx, case_id).await? {
        return Ok(Some(block));
    }
    let handled: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM exhibitions WHERE case_id=? AND status='closed') OR EXISTS(SELECT 1 FROM exhibition_not_required WHERE case_id=?)")
        .bind(case_id)
        .bind(case_id)
        .fetch_one(&mut *tx)
        .await?;
    Ok((!handled).then(|| {
        "Publish the public exhibition and let it close, or record that exhibition is not required for this request with the reason.".into()
    }))
}
/// Legacy definitions marked the exhibition step optional: a skip is only allowed when nothing is open or
/// unconsidered, and it is recorded as an explicit "not required" decision.
pub async fn on_skip(
    tx: &mut SqliteConnection,
    actor: Option<i64>,
    case_id: i64,
    reason: &str,
) -> AppResult<Option<String>> {
    if let Some(block) = case_block(tx, case_id).await? {
        return Ok(Some(block));
    }
    let closed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM exhibitions WHERE case_id=? AND status='closed')")
            .bind(case_id)
            .fetch_one(&mut *tx)
            .await?;
    if !closed {
        record_not_required(tx, actor, case_id, reason, &crate::time::now_str()).await?;
    }
    Ok(None)
}
pub fn display_status(e: &Exhibition) -> &str {
    if e.terminated_at.is_some() { "terminated" } else { &e.status }
}
/// Exhibition state of one case for the building route panel.
pub async fn case_view(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Value> {
    close_due(tx, &crate::time::now_str()).await?;
    let rows: Vec<Exhibition> = sqlx::query_as("SELECT * FROM exhibitions WHERE case_id=? ORDER BY id")
        .bind(case_id)
        .fetch_all(&mut *tx)
        .await?;
    let mut out = vec![];
    for e in &rows {
        let (total, pending): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),COALESCE(SUM(status='received'),0) FROM public_submissions WHERE exhibition_id=?",
        )
        .bind(e.id)
        .fetch_one(&mut *tx)
        .await?;
        out.push(json!({"id":e.id,"title":e.title,"status":display_status(e),"opens_at":e.opens_at,"closes_at":e.closes_at,"terminated_at":e.terminated_at,"termination_reason":e.termination_reason,"consideration_summary":e.consideration_summary,"submissions":total,"pending_submissions":pending}));
    }
    let not_required: Option<(String, String, Option<String>)> = sqlx::query_as("SELECT r.reason,r.decided_at,u.display_name FROM exhibition_not_required r LEFT JOIN users u ON u.id=r.decided_by WHERE r.case_id=? ORDER BY r.id DESC LIMIT 1")
        .bind(case_id)
        .fetch_optional(&mut *tx)
        .await?;
    Ok(
        json!({"exhibitions":out,"not_required":not_required.map(|(reason,at,by)|json!({"reason":reason,"decided_at":at,"decided_by":by})),"block":step_block(tx,case_id).await?}),
    )
}

static RENDERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

async fn redacted_preview(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, item, page)): Path<(i64, i64, String)>,
) -> AppResult<Response> {
    let page = page.strip_suffix(".png").and_then(|s| s.parse::<u32>().ok()).ok_or_else(AppError::not_found)?;
    let mut c = state.db.acquire().await?;
    staff(&mut c, &actor, id).await?;
    let item = items(&mut c, id).await?.into_iter().find(|i| i.id == item).ok_or_else(AppError::not_found)?;
    let (_, _, _, blob, _) = super::uploads::version_access(&mut c, &actor, item.source_document_version_id).await?;
    drop(c);
    let (row, bytes) = storage::read(&state, blob).await?;
    let rects: Vec<Rect> = serde_json::from_str(&item.redactions_json)?;
    let (copy, name) = redacted(&bytes, &row.mime, &rects).await?;
    render_bytes(&copy, if name.ends_with(".pdf") { "application/pdf" } else { "image/png" }, page).await
}
async fn withdraw(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Revision>,
) -> AppResult<Json<Value>> {
    let mut tx = write_tx(&state.db).await?;
    let e = load(&mut tx, id).await?;
    super::manage(&mut tx, &actor, e.case_id, &[Role::Manager]).await?;
    crate::cases::core::bump_revision(&mut tx, e.case_id, Some(input.expected_revision)).await?;
    if !matches!(e.status.as_str(), "open" | "closed") {
        return Err(AppError::conflict("Only a published exhibition can be withdrawn."));
    }
    sqlx::query("UPDATE exhibitions SET status='withdrawn' WHERE id=?").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE exhibition_items SET published_blob_id=NULL WHERE exhibition_id=?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        e.case_id,
        "documents.exhibition_withdrawn",
        Visibility::Applicant,
        "A manager withdrew this exhibition and removed every public copy.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn lookup(State(state): State<AppState>, actor: Actor, Path(number): Path<String>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    let id: i64 = sqlx::query_scalar("SELECT id FROM cases WHERE number=? AND module='building'")
        .bind(number.trim().to_uppercase())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let case = super::manage(&mut tx, &actor, id, &[Role::Specialist, Role::Manager]).await?;
    Ok(Json(json!({"id":id,"number":case.number,"revision":case.revision})))
}
