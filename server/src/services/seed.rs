//! Clearly fictional demo configuration based on researched NIRC forms; amounts owned by finance.
use super::catalog;
use crate::{error::AppResult, state::AppState, time};
use serde::Serialize;
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
/// Building approval route checkpoints (audit N-01/N-03): fee determination at acceptance, then payment.
fn fee_step() -> Value {
    let mut s = step("fees", "module", Some("intake"), "Determine fees", "We are confirming your application fee.");
    s["handler"] = json!("finance.fee_assessed");
    s
}
fn exhibition_step() -> Value {
    let mut s = step(
        "exhibition",
        "module",
        Some("specialist"),
        "Public exhibition and comments",
        "We are deciding on, or running, public exhibition of your proposal.",
    );
    s["handler"] = json!("documents.exhibition_closed");
    s
}
const DA_FEE_NOTE: &str = "Building Development and Works scale on the total estimated cost: $570 up to $50,000; above that $600 + $4.00 per $1,000 over $50,000, with further bands (FY2026-27 demo schedule — confirm with Council). Council records the fee assessment when accepting the application and invoices it before assessment.";
const MOD_FEE_NOTE: &str = "Basic modification (lapse date only) $250; any other modification uses the Building Development and Works scale on the total estimated cost (FY2026-27 demo schedule — confirm with Council). Council records the fee assessment when accepting the application and invoices it before assessment.";
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
fn labelled(key: &str, kind: &str, label: &str, required: bool, options: &[(&str, &str)]) -> Value {
    let mut f = field(key, kind, label, required);
    f["options"] = json!(options.iter().map(|(v, l)| json!({"value":v,"label":l})).collect::<Vec<_>>());
    f
}
fn help(mut f: Value, text: &str) -> Value {
    f["help"] = json!(text);
    f
}
fn shown_when(mut f: Value, field: &str, equals: &str) -> Value {
    f["show_if"] = json!({"field":field,"equals":equals});
    f
}
fn group(key: &str, label: &str, columns: Vec<Value>) -> Value {
    let mut f = field(key, "group", label, true);
    f.as_object_mut().expect("field").remove("max_length");
    f["columns"] = json!(columns);
    f["min_items"] = json!(1);
    f
}
/// Name and contact columns of the official form's applicant and landowner sections.
fn person_columns() -> Vec<Value> {
    vec![
        field("first_name", "text", "First name", true),
        field("last_name", "text", "Last name", true),
        field("postal_address", "textarea", "Postal address", true),
        field("phone", "phone", "Phone", false),
        field("mobile", "phone", "Mobile", false),
        field("email", "email", "Email", false),
    ]
}
/// Application to Modify Development and/or Building Approval (form of 12 March 2024), section by section;
/// mapping and requiredness basis in `docs/forms/modify-approval-mapping.md`. Keeps the approval-routing
/// fields the building workflow owns (`original_approval`, `approvals_sought`) and the shared declaration.
fn modify_approval_form(shared: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
    let kept = |key: &str| shared.iter().find(|f| f["key"] == key).cloned();
    let mut fields: Vec<Value> = ["original_approval", "approvals_sought"].into_iter().filter_map(kept).collect();
    let mut landowners = person_columns();
    landowners.push(field(
        "consent",
        "checkbox",
        "This landowner consents to lodging this modification (signed consent attached)",
        true,
    ));
    fields.extend([
        help(group("applicants", "Applicants", person_columns()), "Section 1. An applicant may be an agent acting on behalf of a landowner. Add a row for each applicant."),
        labelled("landowners_are_applicants", "select", "Are all landowners listed above as applicants?", true, &[("yes", "Yes — every landowner is an applicant"), ("no", "No — list the landowners")]),
        help(shown_when(group("landowners", "Landowners", landowners), "landowners_are_applicants", "no"), "Section 2. Add a row for each landowner. Their signatures are collected on the signed consent you upload under 'Signed consent of all landowners'; staff check it in the Documents tab."),
        help(field("property_ref", "property_ref", "Property address", true), "Section 3: street address of the land."),
        help(group("parcels", "Land parcels", vec![field("portion", "text", "Portion number", true), field("lot", "text", "Lot number", false), field("section", "text", "Section number", false), field("land_area", "text", "Land area (for example 2,000 m² or 1.2 ha)", false)]), "Section 3. Add a row for each portion, lot and section."),
        select("land_tenure", "Land tenure", &["Freehold", "Crown Lease", "Vacant Crown Land", "Road Reserve", "Un-alienated Crown Land"]),
        select("zoning", "Zoning", &["Rural", "Rural Residential", "Residential", "Mixed Use", "Business", "Light Industry", "Industry", "Open Space", "Conservation", "Special Use", "Airport", "Roads"]),
        field("current_use", "textarea", "What is the land currently used for?", true),
        help(labelled("use_types", "multiselect", "Type(s) of use, development and/or building included", true, &[("residential", "Residential (tourist accommodation units, dual occupancy, dwelling house, multiple dwelling)"), ("commercial", "Commercial (e.g. business premises, office, food premises, shop, tourist facility)"), ("industrial", "Industrial (general, light, rural, noxious/hazardous/offensive, extractive)"), ("home_industry", "Home industry or home occupation"), ("alterations_additions", "Alterations and additions to existing structure(s)"), ("ancillary", "Ancillary structures such as garage, verandah, shed"), ("change_of_use", "Change of use"), ("subdivision", "Subdivision (additional lots, boundary adjustment, amalgamation/consolidation)"), ("signage", "Advertising structure and/or signage"), ("community", "Community (e.g. educational establishment, hospital, public building)"), ("infrastructure", "Infrastructure (e.g. public works, electricity, waste, communications, roadworks)"), ("earthworks", "Earthworks (excavation, filling, site works)"), ("other", "Other")]), "Section 4. Tick all relevant boxes."),
        shown_when(field("use_types_other", "text", "Other type of use or development", true), "use_types", "other"),
        help(labelled("modification_types", "multiselect", "Type(s) of modification", true, &[("minor_error", "Minor error, misdescription or miscalculation"), ("conditions", "Modification to condition(s)"), ("lapse_date", "Change of approval lapse date"), ("other", "Any other modification")]), "Section 5. Tick every type that applies; each asks for its own description."),
        shown_when(field("minor_error_description", "textarea", "Minor error: describe the modification and its expected impact", true), "modification_types", "minor_error"),
        shown_when(field("conditions_description", "textarea", "Condition(s): describe the modification and its expected impact", true), "modification_types", "conditions"),
        shown_when(field("proposed_lapse_date", "date", "Proposed approval lapse date", true), "modification_types", "lapse_date"),
        shown_when(field("lapse_date_reasons", "textarea", "Reasons for requiring the change of lapse date", true), "modification_types", "lapse_date"),
        shown_when(field("other_modification_description", "textarea", "Other modification: describe the modification and its expected impact", true), "modification_types", "other"),
        help(field("modified_proposal", "textarea", "The proposed modified use or development, including all modifications made since the original approval", true), "Section 6: helps Council decide whether the modified use or development remains substantially the same as the approved one."),
        field("external_environment_changes", "textarea", "Changes in the external environment since the original approval", true),
        help(field("estimated_cost", "number", "Total estimated cost of building and works (AUD)", true), "Section 7: labour and materials. Council uses it to assess the modification fee."),
        help(labelled("other_approvals", "multiselect", "Other approvals the modification may need", false, &[("epbc", "Environment Protection and Biodiversity Conservation Act 1999 (Cth)"), ("crown_lands", "Crown Lands Act 1996 (NI)"), ("local_government", "Local Government Act 1993 (NSW)(NI)"), ("trees", "Trees Act 1997 (NI)"), ("public_reserves", "Public Reserves Act 1997 (NI)"), ("subdivision", "Subdivision Act 2002 (NI)"), ("tourist_accommodation", "Tourist Accommodation Act 1984 (NI)"), ("sale_of_food", "Sale of Food Act 1950 (NI)"), ("liquor", "Liquor Act 2005 (NI)"), ("heritage", "Heritage Act 2002 (NI)"), ("roads", "Roads Act 2002 (NI)"), ("other", "Other approvals")]), "Section 8. If in doubt, contact the Planning Office."),
        shown_when(field("other_approvals_details", "text", "Other approvals", true), "other_approvals", "other"),
    ]);
    fields.extend(kept("declaration").map(|mut d| {
        d["label"] = json!("I/we, the applicant(s), declare that the information in this application is correct");
        help(d, "Replaces the applicant signatures in section 1 of the paper form.")
    }));
    let documents = vec![
        help(
            doc("title_search", "Copy of title search", true),
            "Section 3: a copy of the Title Search for the subject property.",
        ),
        help(
            shown_when(
                doc("owners_consent", "Signed consent of all landowners", true),
                "landowners_are_applicants",
                "no",
            ),
            "Section 2: every landowner signs to consent to lodging this modification only. Needed only when some landowners are not applicants.",
        ),
        help(
            doc("modification_plans", "Description of expected impacts, with relevant plans and drawings", true),
            "Section 5: a full description of the expected impacts of the proposed modifications, including relevant plans, drawings and compliance with relevant controls.",
        ),
        help(
            doc("supporting", "Other supporting information (plans, drawings, photographs)", false),
            "Section 9: any additional material that shows what is proposed.",
        ),
    ];
    (fields, documents)
}
fn project_field(label: &str) -> Value {
    let mut f = field("project_reference", "project_ref", label, true);
    f["help"] =
        json!("Choose one of your building projects, or type its reference (for example BP-2026-000001) or ID.");
    f
}
fn yes_na(key: &str, label: &str) -> Value {
    let mut f = field(key, "select", label, true);
    f["options"] = json!([{"value":"yes","label":"Yes"},{"value":"n_a","label":"N/a"}]);
    f
}
/// Section 4 "Specified stage" of the official Stage A–E notices, quoting Building Regulation 2004 (NI) Schedule 3.
fn stage_items(stage: &str) -> &'static [&'static str] {
    match stage {
        "A" => &[
            "(a) excavation and placement of formwork and steel reinforcing for any slab, footing, wall, foundation wall, or like item but before any concrete for any slab, footing, wall, foundation wall or like item is poured",
            "(b) excavation for any piers, posts, stumps, and like items but before any concrete for any piers, posts, stumps, or like items is poured",
            "(c) placement of formwork and steel for any reinforced concrete member but before any concrete for the member is poured",
        ],
        "B" => &[
            "completion of the structural framework and before the placement of any external cladding, roofing material or internal lining",
        ],
        "C" => &["completion of drainage work but before the covering over of any of those works"],
        "D" => &["completion of plumbing services but before the covering over of any of those services"],
        _ => &[
            "completion of the building work approved in the relevant building approval before occupancy or use of the building",
        ],
    }
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
        (
            "builder-stage-a-notice",
            "Builder's Stage A Compliance Declaration Notice",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/builders_stage_a_compliance_declaration_notice.pdf",
        ),
        (
            "builder-stage-b-notice",
            "Builder's Stage B Compliance Declaration Notice",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/builders_stage_b_compliance_declaration_notice.pdf",
        ),
        (
            "builder-stage-c-notice",
            "Builder's Stage C Compliance Declaration Notice",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/builders_stage_c_compliance_declaration_notice.pdf",
        ),
        (
            "builder-stage-d-notice",
            "Builder's Stage D Compliance Declaration Notice",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/builders_stage_d_compliance_declaration_notice.pdf",
        ),
        (
            "builder-stage-e-notice",
            "Builder's Stage E Compliance Declaration Notice",
            "Planning & Building",
            "building",
            "Planning",
            "planning-development/documents/builders_stage_e_compliance_declaration_notice.docx",
        ),
        (
            "pipeline-conduit-crossing",
            "Application to Install Pipeline or Conduit Crossing in Public Roadway",
            "Works & Roads",
            "generic",
            "Works Depot",
            "infrastructure/documents/212_application_to_install_pipeline_or_conduit_crossing_in_public_roadway.pdf",
        ),
    ];
    entries.into_iter().map(|(slug,name,category,m,department,url)|{
        let mut fields=contact();let mut docs=vec![];let mut pricing=vec![];let mut conditions=vec![];
        let (mut response_days,mut fee_note,mut own_declaration)=(20,None::<&str>,false);
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
                    let mut original=field("original_approval","decision_ref","Issued approval(s) to modify — choose the development and/or building approval",true);original["multiple"]=json!(true);
                    fields.push(original);
                    steps=vec![intake.clone(),fee_step(),payment(),assessment.clone(),exhibition_step(),decision(&["modification_approval"]),done()];
                }else{
                    fields.extend([field("gross_floor_area","number","Gross floor area (square metres)",false),field("roof_area","number","Total roof area (square metres)",false),field("water_tank_litres","number","Total water storage (litres)",true),select("wastewater","Wastewater disposal",&["Sewer connection","Onsite system"]),field("earthworks_volume","number","Earthworks (cubic metres)",false),field("builder","text","Builder's details",false)]);
                    let mut sought=field("approvals_sought","multiselect","Approvals sought",true);sought["options"]=json!([{"value":"development_approval","label":"Development approval"},{"value":"building_approval","label":"Building approval"}]);sought["help"]=json!("Council confirms which approvals your proposal needs.");
                    let at=fields.iter().position(|f|f["key"]=="current_use").expect("current_use");fields.insert(at,sought);
                    docs.push(doc("clause12","Clause 12 Norfolk Island Plan checklist",true));steps=vec![intake.clone(),fee_step(),payment(),assessment.clone(),exhibition_step(),decision(&["development_approval","building_approval"]),done()];
                }
                if slug=="modify-approval"{("Apply to modify an issued approval with landowner consent and supporting plans.","A modification decision linked to the original approval.",vec!["development","building","modification","approval"])}else{("Apply for planning and building decisions with landowner consent, plans and supporting information.","Separate development and building decisions; approval is required before relevant work starts.",vec!["development","building","permit","DA","plans"])}
            },
            "building-commencement-notice"|"building-completion-notice"=>{
                fields.extend(property());fields.push(project_field("Building project reference or ID"));
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
            s if s.starts_with("builder-stage-")=>{
                // Builder's Stage A–E Compliance Declaration Notices (s34 Building Act 2002 (NI)), updated 24 February 2025.
                let stage=s[14..15].to_uppercase();
                fields=vec![project_field("Building approval — your building project"),field("builder_first_name","text","Person who carried out the building work — first name",true),field("builder_last_name","text","Person who carried out the building work — last name",true),field("phone","phone","Phone number",true)];
                if stage=="A"{fields.push(field("mobile","phone","Mobile number",true));}
                fields.extend([field("email","email",if stage=="A"{"Email(s)"}else{"Email"},true),field("property_ref","property_ref","Portion number",true),field("address","text","Property address",true)]);
                fields.extend([field("declare_completed","checkbox",&format!("I have completed the building work described for Inspection Stage {stage}"),true),field("declare_complies","checkbox",&format!("I declare that the building work complies with the relevant and applicable requirements for Stage {stage} building work specified in Schedule 3 of the Building Regulation 2004 (NI)"),true),field("declare_approval","checkbox","I certify that I am satisfied that the building work has been completed in accordance with the building approval and the Building Act 2002 (NI)",true),field("declaration_date","date","Date of the compliance declaration",true)]);
                let items=stage_items(&stage);
                for (i,item) in items.iter().enumerate(){
                    let key=if items.len()==1{"stage_item".to_string()}else{format!("stage_item_{}",(b'a'+i as u8) as char)};
                    fields.push(yes_na(&key,&format!("Specified Stage {stage} work (Building Regulation 2004 (NI) Schedule 3): {item}")));
                }
                docs=vec![doc("declaration",&format!("Signed Builder's Stage {stage} compliance declaration notice"),true)];
                pricing.push(json!({"item":"BUILDING_STAGE_INSPECTION","quantity":1}));
                let mut accept=step("decision","decision",Some("specialist"),"Accept declaration and permit work to continue","We are confirming whether building work may continue.");
                accept["decision_types"]=json!(["service_response"]);
                steps=vec![intake.clone(),payment(),task("site","site_inspection",&format!("Stage {stage} inspection by an authorised officer"),false),accept,done()];
                fee_note=Some("Building inspection fee: $83.00 per stage (Council fees and charges; demo schedule — confirm before real use).");
                own_declaration=true;
                ("Notify the General Manager under section 34 of the Building Act 2002 (NI) that this stage of building work complies with the building approval, so that an authorised officer can inspect it.","An inspection and written permission under section 34(c) to continue building work, recorded on the same building project.",vec!["building","stage","inspection","compliance","declaration","notice"])
            },
            "pipeline-conduit-crossing"=>{
                // Form 212, last updated 1 June 2023: every field except the business name is marked mandatory.
                for f in fields.iter_mut(){if matches!(f["key"].as_str(),Some("email"|"phone")){f["required"]=json!(true);}}
                fields.extend([field("organisation_name","text","Business / company name",false),field("abn","text","ABN / ACN number",true),field("position_held","text","Position held by the person signing",true),field("road_name","text","Name of road that the pipeline or conduit will be installed in",true),field("crossing_location","property_ref","Location of proposed pipeline or conduit crossing (Portion number of property)",true),field("pipe_size_type","textarea","Size and type of proposed pipe or conduit",true),field("adjacent_portions","text","Description of land adjacent to the road reserve (Portion numbers)",true),field("no_work","checkbox","I/we understand that no work is to be carried out until this application is approved by Council",true),field("restoration","checkbox","I/we agree to meet the costs of restoring the road pavement, or other remedial work Council deems necessary",true)]);
                docs.push(doc("drawing","Sketch or drawing showing location, dimensions and levels of the proposed pipe or conduit",true));
                steps=vec![intake.clone(),review("assessment","intake","Assessing the pipeline or conduit crossing"),done()];
                response_days=10;
                fee_note=Some("Form 212 does not state an application fee. Council will confirm any fee and road restoration costs before work starts.");
                ("Apply to install a pipeline or other conduit within a road reserve or beneath a public roadway. An officer responds within 10 working days.","A written decision approving or declining the proposed crossing.",vec!["pipe","pipeline","conduit","crossing","road","water","cable"])
            },
            _=>{fields.extend([field("dog_name","text","Dog's name",true),field("breed","text","Breed",true),field("microchip","text","Microchip number",false)]);("Draft demonstration service: dog registration rules need review before publishing.","A registration confirmation after staff review.",vec!["dog","registration"])},
        };
        if m=="generic"&&slug!="dog-registration"{let terminal=steps.pop().expect("complete");steps.push(decision(&["service_response"]));steps.push(terminal);}
        if !own_declaration{fields.push(field("declaration","checkbox","I declare that the information is correct",true));}
        if slug=="modify-approval"{(fields,docs)=modify_approval_form(fields);}
        let first=steps.first().unwrap()["key"].as_str().unwrap();let second=steps.get(1).unwrap()["key"].as_str().unwrap();
        let deadlines=json!([{"kind":"completeness","label":"Initial check","days":if m=="building"{10}else{3},"basis":"business","starts":"submitted","stops":format!("step:{second}"),"pausable":false},{"kind":"response","label":"Reply","days":response_days,"basis":"business","starts":format!("step:{first}"),"stops":"closed","pausable":true,"max_pause_days":30}]);
        let calculation=match m {
            "venue_booking"=>"Hire fee plus a refundable bond. Hall fees are charged per calendar day touched by the booking.",
            "equipment_hire"=>"Final charges use actual approved minutes and any agreed pass-through expenses. Rates include fuel, oil and the operator's ordinary-time wages.",
            _=>"Council will confirm the fee before payment.",
        };
        let price_note=match slug {"road-issue"|"complaint"=>"No fee applies.","development-application"=>DA_FEE_NOTE,"modify-approval"=>MOD_FEE_NOTE,_=>fee_note.unwrap_or(calculation)};
        let building_role=match slug{"development-application"=>Some("project"),"modify-approval"=>Some("modification"),_ if m=="building"=>Some("follow_up"),_=>None};
        let mut def=json!({"module":m,"summary":summary,"conditions":conditions,"outcome":outcome,"who_can_apply":"Residents and businesses, or an authorised representative.","price_note":price_note,"keywords":keywords,"fields":fields,"documents":docs,"workflow":{"steps":steps},"deadlines":deadlines,"pricing":pricing});
        if let Some(role)=building_role{def["building_role"]=json!(role);}
        let source=if url.is_empty(){"https://www.nirc.gov.au/Customer-Service/Customer-Service-Forms".into()}else{format!("{ROOT}{url}")};
        (slug,name,category,m,department,def,source)
    }).collect()
}
/// What [`upgrade`] did for one catalogue entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogueAction {
    /// The service did not exist and was created from the seed.
    Created,
    /// The published version came from the seed; a new seed version was published and the old one retired.
    Upgraded,
    /// Staff changed the published version; the new seed definition was stored as a draft for review.
    DraftForReview,
    /// A draft with the current seed definition already waits for staff review.
    AwaitingReview,
    Unchanged,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogueChange {
    pub slug: &'static str,
    pub action: CatalogueAction,
    /// The version created, published or found.
    pub version: i64,
    /// The previously published version, when it was retired or kept for staff.
    pub previous: Option<i64>,
}
impl std::fmt::Display for CatalogueChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.action, self.previous) {
            (CatalogueAction::Created, _) => write!(f, "{}: created version {}", self.slug, self.version),
            (CatalogueAction::Upgraded, Some(p)) => {
                write!(f, "{}: published version {} (version {p} retired)", self.slug, self.version)
            }
            (CatalogueAction::DraftForReview, p) => write!(
                f,
                "{}: published version {} was edited by staff; seed changes saved as draft version {} for review",
                self.slug,
                p.map_or("-".into(), |p| p.to_string()),
                self.version
            ),
            (CatalogueAction::AwaitingReview, _) => {
                write!(f, "{}: seed draft version {} awaits staff review", self.slug, self.version)
            }
            _ => write!(f, "{}: unchanged (version {})", self.slug, self.version),
        }
    }
}
/// SHA-256 of the canonical seeded definition JSON (`service_versions.seed_hash`).
pub fn seed_hash(definition: &Value) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(definition.to_string().as_bytes()))
}
type VersionRow = (i64, i64, String, String, Option<String>);
/// Seed provenance: the stored content still hashes to the recorded seed hash. Versions seeded before
/// migration 0803 carry `legacy`, trusted only for immutable (non-draft) versions.
fn seeded(v: &VersionRow) -> bool {
    match v.4.as_deref() {
        Some("legacy") => v.2 != "draft",
        Some(h) => serde_json::from_str::<Value>(&v.3).is_ok_and(|d| seed_hash(&d) == h),
        None => false,
    }
}
/// One seeded version to write.
struct Seeded<'a> {
    definition: &'a Value,
    hash: &'a str,
    note: &'a str,
    now: &'a str,
}
impl Seeded<'_> {
    async fn write(&self, tx: &mut SqliteConnection, id: i64, version: i64, status: &str) -> AppResult<()> {
        sqlx::query("INSERT INTO service_versions(service_id,version,status,definition_json,source_note,created_at,published_at,seed_hash) VALUES (?,?,?,?,?,?,?,?)")
            .bind(id).bind(version).bind(status).bind(self.definition.to_string()).bind(self.note).bind(self.now)
            .bind((status == "published").then_some(self.now)).bind(self.hash).execute(&mut *tx).await?;
        catalog::reindex(tx, id).await
    }
}
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    for change in upgrade(tx, state).await? {
        if change.action != CatalogueAction::Unchanged {
            tracing::info!(%change, "service catalogue");
        }
    }
    Ok(())
}
/// Installs or upgrades the seeded catalogue. Missing services are created. A published version that still
/// carries seed provenance and differs from the current seed is retired in favour of a new published version;
/// a published version edited by staff is never overwritten: the seed definition becomes a draft for review.
/// Submitted cases keep the version they were created on and their frozen submission snapshot; drafts move to
/// the new published version on their next save or submission (`cases::drafts::rebind`). Idempotent.
pub async fn upgrade(tx: &mut SqliteConnection, state: &AppState) -> AppResult<Vec<CatalogueChange>> {
    sqlx::query("INSERT INTO service_synonyms VALUES ('party','hall'),('birthday','hall'),('wedding','hall'),('venue','hall'),('digger','equipment'),('excavator','equipment'),('pothole','road'),('hole','road'),('da','development'),('permit','development'),('pipe','pipeline'),('conduit','pipeline') ON CONFLICT DO NOTHING").execute(&mut *tx).await?;
    let now = time::fmt(state.now());
    let mut changes = Vec::new();
    for (slug, name, category, module, department, definition, source) in catalogue() {
        let hash = seed_hash(&definition);
        let note = format!(
            "Fictional demonstration configuration based on NIRC form: {source}. Every rule must be checked before real use."
        );
        let target = if slug == "dog-registration" { "draft" } else { "published" };
        let insert = Seeded { definition: &definition, hash: &hash, note: &note, now: &now };
        let change = |action, version, previous| CatalogueChange { slug, action, version, previous };
        let service: Option<i64> =
            sqlx::query_scalar("SELECT id FROM services WHERE slug=?").bind(slug).fetch_optional(&mut *tx).await?;
        let Some(id) = service else {
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO services(slug,name,category,module,department,created_at) VALUES (?,?,?,?,?,?) RETURNING id",
            )
            .bind(slug)
            .bind(name)
            .bind(category)
            .bind(module)
            .bind(department)
            .bind(&now)
            .fetch_one(&mut *tx)
            .await?;
            insert.write(tx, id, 1, target).await?;
            changes.push(change(CatalogueAction::Created, 1, None));
            continue;
        };
        let versions: Vec<VersionRow> = sqlx::query_as(
            "SELECT id,version,status,definition_json,seed_hash FROM service_versions WHERE service_id=? ORDER BY version",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let next = versions.iter().map(|v| v.1).max().unwrap_or(0) + 1;
        let same = |v: &VersionRow| serde_json::from_str::<Value>(&v.3).is_ok_and(|d| d == definition);
        let published = versions.iter().find(|v| v.2 == "published");
        // The seed definition is already in place: stamp provenance on a matching legacy version.
        if let Some(v) = versions.iter().find(|v| v.2 == target && same(v)) {
            if seeded(v) && v.4.as_deref() != Some(hash.as_str()) {
                sqlx::query("UPDATE service_versions SET seed_hash=? WHERE id=?")
                    .bind(&hash)
                    .bind(v.0)
                    .execute(&mut *tx)
                    .await?;
            }
            changes.push(change(CatalogueAction::Unchanged, v.1, None));
            continue;
        }
        if target == "published"
            && let Some(p) = published.filter(|p| seeded(p))
        {
            sqlx::query("UPDATE service_versions SET status='retired' WHERE id=?").bind(p.0).execute(&mut *tx).await?;
            insert.write(tx, id, next, "published").await?;
            changes.push(change(CatalogueAction::Upgraded, next, Some(p.1)));
            continue;
        }
        if let Some(v) = versions.iter().find(|v| v.2 == "draft" && v.4.as_deref() == Some(hash.as_str()) && seeded(v))
        {
            changes.push(change(CatalogueAction::AwaitingReview, v.1, published.map(|p| p.1)));
            continue;
        }
        insert.write(tx, id, next, "draft").await?;
        tracing::warn!(
            slug,
            version = next,
            "service catalogue: the current version is not seed-managed; seed changes saved as a draft for staff review"
        );
        changes.push(change(CatalogueAction::DraftForReview, next, published.map(|p| p.1)));
    }
    Ok(changes)
}
