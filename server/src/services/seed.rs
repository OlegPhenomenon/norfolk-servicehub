//! Clearly fictional demo configuration based on researched NIRC forms; amounts owned by finance.
use super::catalog;
use crate::{error::AppResult, state::AppState, time};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
const ROOT: &str = "https://www.nirc.gov.au/files/assets/public/v/1/";
fn field(key: &str, kind: &str, label: &str, required: bool) -> Value {
    json!({"key":key,"type":kind,"label":label,"required":required,"max_length":2000})
}
fn select(key: &str, label: &str, options: &[&str]) -> Value {
    let mut f = field(key, "select", label, true);
    f["options"] = json!(options.iter().map(|o| json!({"value":o,"label":o})).collect::<Vec<_>>());
    f
}
fn doc(key: &str, label: &str, required: bool) -> Value {
    json!({"key":key,"label":label,"required":required,"accept":["application/pdf","image/png","image/jpeg"],"public_candidate":false})
}
fn staff_label<'a>(key: &str, fallback: &'a str) -> &'a str {
    match key {
        "assessment" if fallback == "Finding the requested record" => "Find requested record",
        "triage" if fallback == "Reviewing your complaint" => "Review complaint",
        "intake" => "Check request",
        "assessment" => "Assess application",
        "confirm" => "Confirm booking",
        "prep" => "Prepare hall",
        "inspect" => "Inspect hall",
        "bond" => "Settle bond",
        "schedule" => "Schedule equipment",
        "job" => "Complete equipment job",
        "usage" => "Approve actual usage",
        "triage" => "Triage report",
        "investigation" => "Investigate complaint",
        "inspection" => "Inspect road issue",
        "site" => "Inspect site",
        "preparation" => "Prepare certificate",
        "repair" => "Repair road",
        "response" => "Issue response",
        _ => fallback,
    }
}
fn step(key: &str, kind: &str, role: Option<&str>, label: &str, applicant: &str) -> Value {
    let mut s = json!({"key":key,"kind":kind,"label":staff_label(key, label),"applicant_label":applicant});
    if let Some(r) = role {
        s["role"] = json!(r);
    }
    s
}
fn review(key: &str, role: &str, label: &str) -> Value {
    step(key, "review", Some(role), label, &format!("We are {}.", label.to_lowercase()))
}
fn task(key: &str, kind: &str, label: &str, optional: bool) -> Value {
    let mut s = step(key, "task", Some(if optional { "intake" } else { "field_worker" }), label, label);
    s["task_kind"] = json!(kind);
    s["optional"] = json!(optional);
    s
}
fn module(key: &str, handler: &str, role: &str, label: &str, optional: bool) -> Value {
    let mut s = step(key, "module", Some(role), label, label);
    s["handler"] = json!(handler);
    s["optional"] = json!(optional);
    s
}
fn decision(types: &[&str]) -> Value {
    let mut s =
        step("decision", "decision", Some("specialist"), "Issue decisions", "We are preparing your decision document.");
    s["decision_types"] = json!(types);
    s
}
fn payment() -> Value {
    step("payment", "payment", Some("finance"), "Receive payment", "Payment is needed before we can continue.")
}
fn done() -> Value {
    step("done", "complete", None, "Completed", "Your request is complete.")
}
fn contact() -> Vec<Value> {
    vec![
        field("applicant_name", "text", "Name of applicant", true),
        field("postal_address", "textarea", "Postal address", true),
        field("email", "email", "Email address", false),
        field("phone", "phone", "Phone (work or mobile)", false),
    ]
}
fn property() -> Vec<Value> {
    vec![
        field("property_ref", "property_ref", "Portion number and street address", true),
        field("lot", "text", "Lot number", false),
        field("section", "text", "Section number", false),
        field("owner_name", "text", "Landowner's name", true),
    ]
}
pub fn catalogue() -> Vec<(&'static str, &'static str, &'static str, &'static str, &'static str, Value, String)> {
    let entries = [
        (
            "rawson-hall-hire",
            "Application for Hire of a Council Premises — Rawson Hall",
            "Venues",
            "venue_booking",
            "Customer Care",
            "customer-service/documents/application_for_hire_of_a_council_premises.pdf",
        ),
        (
            "development-application",
            "Application for Development and/or Building Approval",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/application_for_da_ba_approval_form_05_09_24.pdf",
        ),
        (
            "modify-approval",
            "Application to Modify Development and/or Building Approval",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/application_to_modify_development_and_or_building_approval_form_18_03_24.pdf",
        ),
        (
            "building-commencement-notice",
            "Commencement of Building Work — 48 Hours Prior Notice",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/commencement_of_building_work_48_hours_prior_notice.pdf",
        ),
        (
            "building-completion-notice",
            "Completion of Building Work Compliance Declaration",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/completion_of_building_work_compliance_declaration.pdf",
        ),
        (
            "planning-certificate",
            "Planning Certificate — Section 98 Planning Act 2002",
            "Planning & Building",
            "planning_certificate",
            "Planning",
            "planning-development/documents/application_for_planning_certificate_section_98_planning_act_2002.pdf",
        ),
        (
            "equipment-hire",
            "Application for Hire of Council Plant / Equipment",
            "Works & Roads",
            "equipment_hire",
            "Works Depot",
            "infrastructure/documents/application_for_hire_of_council_equipment.pdf",
        ),
        (
            "driveway-crossover",
            "Application to Construct Driveway Access Entrance from a Public Roadway",
            "Works & Roads",
            "generic",
            "Works Depot",
            "infrastructure/documents/209_works_depot___application_to_construct_driveway_access_entrance_from_a_public_roadway.pdf",
        ),
        ("road-issue", "Report a Road Issue", "Works & Roads", "road_issue", "Works Depot", ""),
        (
            "complaint",
            "Make a Complaint",
            "Feedback",
            "complaint",
            "Governance",
            "customer-service/documents/complaints___nirc_customer_complaints_form_.pdf",
        ),
        ("council-record-copy", "Request a Council Record Copy", "Information", "generic", "Customer Care", ""),
        (
            "dog-registration",
            "Dog Registration",
            "Animals",
            "generic",
            "Customer Care",
            "customer-service/documents/application_to_register_dogs.pdf",
        ),
    ];
    entries.into_iter().map(|(slug,name,category,m,department,url)|{
        let mut fields=contact();let mut docs=vec![];let mut pricing=vec![];let mut conditions=vec![];
        let intake=review("intake","intake","Checking your request");
        let assessment=review("assessment","specialist","Assessing your application");
        let mut steps=vec![intake.clone(),assessment.clone(),done()];
        let (summary,outcome,keywords)=match slug {
            "rawson-hall-hire"=>{
                fields.extend([field("organisation_name","text","Organisation name",false),select("organisation_type","Type of organisation",&["Private / Individual","Non-Profit","For Profit / Commercial"]),field("purchase_order","text","Business purchase order number",false),field("event_name","text","Event title and type",true),field("slot","booking_slot","Date, time, space and number of guests",true),field("setup_notes","textarea","Other instructions / intended use",false),field("key_collector","text","Name of person collecting key",true),select("alcohol","Will alcohol be served?",&["no","yes"])]);
                let mut permit=field("liquor_permit","text","Liquor permit number",true);permit["show_if"]=json!({"field":"alcohol","equals":"yes"});fields.push(permit);
                fields.extend([field("insurance_agreement","checkbox","Public liability policy or Council casual hirer agreement is attached",true),field("conditions","checkbox","I accept the Conditions of Hire and responsibility for the key",true)]);
                docs=vec![doc("insurance","Public liability policy ($20 million) or Council casual hirer agreement",true),doc("nonprofit","Non-profit supporting documents, if applicable",false)];
                steps=vec![intake.clone(),payment(),module("confirm","operations.booking_confirmed","intake","Confirming your booking",false),task("prep","venue_prep","Preparing the hall",false),task("inspect","venue_inspection","Inspecting the hall after your event",false),module("bond","finance.deposits_settled","finance","Processing your bond decision and refund",false),done()];
                conditions=vec!["A submitted request is not a confirmed booking.","Music stops by 10 pm unless agreed.","Return keys and property by noon on the next business day.","Provide $20 million public liability insurance or agreed casual-hirer cover.","Meeting cancellations need more than seven days notice; weddings, concerts, stage shows and balls need thirty days notice."];
                ("Hire the Main Hall, Supper Room or both for an event.","A confirmed booking with rooms, dates and conditions, followed by a bond decision.",vec!["hall","party","wedding","venue","room hire"])
            },
            "development-application"|"modify-approval"=>{
                fields.extend(property());fields.extend([select("land_tenure","Land tenure",&["Freehold","Crown Lease","Vacant Crown Land","Road Reserve","Un-alienated Crown Land"]),select("zoning","Zoning",&["Rural","Rural Residential","Residential","Mixed Use","Business","Light Industry","Industry","Open Space","Conservation","Special Use","Airport","Roads"]),field("current_use","textarea","What is the land currently used for?",true),field("proposal","textarea",if slug=="modify-approval"{"Description of proposed modification"}else{"Description of proposal"},true),field("estimated_cost","number","Total estimated cost of building and works (AUD)",true),field("owners_consent","checkbox","All landowners consent to lodging this application",true)]);
                docs=vec![doc("title_search","Copy of title search",true),doc("owners_consent","Signed consent of all landowners",true),doc("site_plan","Site plan (at least 1:500)",true),doc("floor_plans","Plans and drawings (floor plans at least 1:100)",true),doc("environment","Environmental and heritage impact documentation",true),doc("earthworks","Earthworks plan, if earthworks exceed 50 cubic metres",false)];
                if slug=="modify-approval"{
                    fields.push(field("original_approval","decision_ref","Issued approval to modify",true));fields.extend([select("modification_type","Type of modification",&["Minor error","Conditions","Lapse date","Other"]),field("substantially_same","textarea","Explain why the use or development remains substantially the same",true)]);
                    pricing.push(json!({"item":"MODIFICATION_FEE","quantity":1}));steps=vec![intake.clone(),assessment.clone(),decision(&["modification_approval"]),done()];
                }else{
                    fields.extend([field("gross_floor_area","number","Gross floor area (square metres)",false),field("roof_area","number","Total roof area (square metres)",false),field("water_tank_litres","number","Total water storage (litres)",true),select("wastewater","Wastewater disposal",&["Sewer connection","Onsite system"]),field("earthworks_volume","number","Earthworks (cubic metres)",false),field("builder","text","Builder's details",false)]);
                    docs.push(doc("clause12","Clause 12 Norfolk Island Plan checklist",true));pricing.extend([json!({"item":"DA_LODGEMENT","quantity":1}),json!({"item":"BA_LODGEMENT","quantity":1})]);steps=vec![intake.clone(),assessment.clone(),module("exhibition","documents.exhibition_closed","specialist","Public exhibition and comments",true),decision(&["development_approval","building_approval"]),done()];
                }
                if slug=="modify-approval"{("Apply to modify an issued approval with landowner consent and supporting plans.","A modification decision linked to the original approval.",vec!["development","building","modification","approval"])}else{("Apply for planning and building decisions with landowner consent, plans and supporting information.","Separate development and building decisions; approval is required before relevant work starts.",vec!["development","building","permit","DA","plans"])}
            },
            "building-commencement-notice"|"building-completion-notice"=>{
                fields.extend(property());fields.push(field("project_reference","text","Building project reference or ID",true));
                if slug=="building-commencement-notice"{fields.push(field("commencement_date","date","Commencement date (notify at least 48 hours before work)",true));}
                else{fields.extend([field("builder","text","Person who carried out the building work",true),field("structure_owner","text","Structure owner's name",true),select("completion","Compliance declaration",&["Fully completed — s38(1)","Partly carried out — s38(2)"]),field("work_description","textarea","Description of work",true)]);docs.push(doc("compliance","Signed building compliance declaration",true));}
                steps=vec![intake.clone(),task("site","site_inspection","Site inspection, if required",true),done()];
                ("Notify Council about an existing approved building project.","Notice recorded against the same building project and an inspection when needed.",vec!["building","commencement","completion","notice"])
            },
            "planning-certificate"=>{fields.extend(property());fields.push(field("sections","textarea","Certificate sections requested",false));docs.push(doc("receipt","Payment receipt (if already paid)",false));pricing.push(json!({"item":"PLANNING_CERT","quantity":1}));steps=vec![intake.clone(),payment(),review("preparation","specialist","Preparing your certificate"),decision(&["planning_certificate"]),done()];("Request a certificate under section 98 of the Planning Act 2002 for each address, including adjoining lots.","An issued planning certificate.",vec!["planning","certificate","property","section 98"])},
            "equipment-hire"=>{fields.extend([field("request","equipment_request","Plant / equipment, purpose, requested hours, date and site",true),field("organisation_name","text","Business / company name",false),field("abn","text","ABN / ACN number",false),field("indemnity","checkbox","I accept the hiring conditions and indemnity declaration",true)]);steps=vec![intake.clone(),module("schedule","operations.equipment_scheduled","intake","Scheduling equipment and operator",false),task("job","equipment_job","Carrying out the equipment job",false),module("usage","operations.usage_invoiced","intake","Calculating actual billable hours",false),payment(),done()];("Hire Council equipment and an operator. Actual hours are recorded from leaving the depot until return; agreed expenses may apply.","A scheduled job and an invoice based on approved actual usage.",vec!["equipment","plant","digger","excavator","hire"])},
            "driveway-crossover"=>{fields.extend(property());fields.extend([field("road_name","text","Name of road",true),field("access_description","textarea","Size of area and type of proposed access or entrance",true),field("adjacent_portions","text","Description of adjacent land (Portion numbers)",true),field("no_work","checkbox","I will not start work until Council approves it",true),field("restoration","checkbox","I meet the costs of pavement restoration and remedial work",true)]);docs.push(doc("sketch","Sketch showing location, dimensions and levels",true));pricing.push(json!({"item":"DRIVEWAY_APPLICATION","quantity":1}));steps=vec![intake.clone(),payment(),review("assessment","intake","Assessing driveway access"),done()];("Request permission for a driveway entrance from a public road. Approval does not mean Council will perform the work.","A response approving or declining the proposed access.",vec!["driveway","crossover","road","entrance"])},
            "road-issue"=>{fields.extend([field("location","location","Location and description of road issue",true),field("description","textarea","Describe the pothole, culvert or other issue",true)]);docs.push(doc("photo","Photograph of the issue",false));steps=vec![review("triage","intake","Checking your road report"),task("inspection","road_inspection","Inspecting the road issue",false),task("repair","road_repair","Repair work, if required",true),module("response","documents.letter_issued:road_response","intake","Preparing your written response",false),done()];("Report a pothole, damaged road, culvert or runoff problem. Personal details remain private.","An inspection, repair when needed, and a written response.",vec!["road","pothole","culvert","repair"])},
            "complaint"=>{let mut subject=field("staff_member_concerned","select","Staff member concerned (if known)",false);subject["options"]=json!([]);fields.push(subject);fields.extend([field("complaint","textarea","Describe your complaint",true),field("desired_outcome","textarea","What outcome would you like?",true)]);docs.push(doc("evidence","Supporting evidence",false));steps=vec![review("triage","complaints_officer","Reviewing your complaint"),review("investigation","complaints_officer","Investigating your complaint"),module("response","documents.letter_issued:complaint_response","complaints_officer","Preparing your complaint response",false),done()];("Make a confidential complaint to the Complaints Officer.","A written complaint response.",vec!["complaint","feedback","confidential"])},
            "council-record-copy"=>{fields.extend([field("record_description","textarea","Describe the Council record requested",true),field("reference","text","Meeting date or record reference",false)]);pricing.push(json!({"item":"RECORD_COPY","quantity":1}));steps=vec![intake.clone(),payment(),review("assessment","intake","Finding the requested record"),done()];("Request a copy of a Council record, such as meeting minutes.","A copy of an available record or an explanation.",vec!["record","copy","minutes","information"])},
            _=>{fields.extend([field("dog_name","text","Dog's name",true),field("breed","text","Breed",true),field("microchip","text","Microchip number",false)]);("Draft demonstration service: dog registration rules need review before publishing.","A registration confirmation after staff review.",vec!["dog","registration"])},
        };
        fields.push(field("declaration","checkbox","I declare that the information is correct",true));
        let first=steps.first().unwrap()["key"].as_str().unwrap();let second=steps.get(1).unwrap()["key"].as_str().unwrap();
        let deadlines=json!([{"kind":"completeness","label":"Initial check","days":if m=="building"{10}else{3},"basis":"business","starts":"submitted","stops":format!("step:{second}"),"pausable":false},{"kind":"response","label":"Reply","days":20,"basis":"business","starts":format!("step:{first}"),"stops":"closed","pausable":true,"max_pause_days":30}]);
        let calculation=match m {
            "venue_booking"=>"Hire fee plus a refundable bond. Hall fees are charged per calendar day touched by the booking.",
            "equipment_hire"=>"Final charges use actual approved minutes and any agreed pass-through expenses. Rates include fuel, oil and the operator's ordinary-time wages.",
            _=>"Council will confirm the fee before payment.",
        };
        let price_note=if matches!(slug,"road-issue"|"complaint"){"No fee applies."}else{calculation};
        let def=json!({"module":m,"summary":summary,"conditions":conditions,"outcome":outcome,"who_can_apply":"Residents and businesses, or an authorised representative.","price_note":price_note,"keywords":keywords,"fields":fields,"documents":docs,"workflow":{"steps":steps},"deadlines":deadlines,"pricing":pricing});
        let source=if url.is_empty(){"https://www.nirc.gov.au/Customer-Service/Customer-Service-Forms".into()}else{format!("{ROOT}{url}")};
        (slug,name,category,m,department,def,source)
    }).collect()
}
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    sqlx::query("INSERT INTO service_synonyms VALUES ('party','hall'),('birthday','hall'),('wedding','hall'),('venue','hall'),('digger','equipment'),('excavator','equipment'),('pothole','road'),('hole','road'),('da','development'),('permit','development') ON CONFLICT DO NOTHING").execute(&mut *tx).await?;

    for (slug, name, category, module, department, definition, source) in catalogue() {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM services WHERE slug=?)")
            .bind(slug)
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            continue;
        }
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO services(slug,name,category,module,department,created_at) VALUES (?,?,?,?,?,?) RETURNING id",
        )
        .bind(slug)
        .bind(name)
        .bind(category)
        .bind(module)
        .bind(department)
        .bind(time::fmt(state.now()))
        .fetch_one(&mut *tx)
        .await?;
        let mut definition = definition;
        if module == "generic" && slug != "dog-registration" {
            let steps = definition["workflow"]["steps"].as_array_mut().expect("steps");
            let terminal = steps.pop().expect("complete");
            steps.push(decision(&["service_response"]));
            steps.push(terminal);
        }
        let status = if slug == "dog-registration" { "draft" } else { "published" };
        sqlx::query("INSERT INTO service_versions(service_id,version,status,definition_json,source_note,created_at,published_at) VALUES (?,1,?,?,?,?,?)").bind(id).bind(status).bind(definition.to_string()).bind(format!("Fictional demonstration configuration based on NIRC form: {source}. Every rule must be checked before real use.")).bind(time::fmt(state.now())).bind((status=="published").then(||time::fmt(state.now()))).execute(&mut *tx).await?;
        catalog::reindex(tx, id).await?;
    }
    Ok(())
}
