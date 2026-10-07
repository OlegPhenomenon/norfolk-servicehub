# Norfolk Island Context

## Public holidays

**Legal basis.** Employment Act 1988 (NI), s9(1), compilation as at 1 July 2016: https://www.legislation.gov.au/C2015Q00125/2016-07-01/2016-07-01/text/original/epub/OEBPS/document_1/document_1.html. The public holidays are:
- (a) 1 January
- (b) 26 January
- (c) 6 March
- (d) 25 April
- (e) Good Friday
- (f) the Monday after Good Friday
- (g) 8 June
- (h) the last Wednesday in November
- (i) Christmas Day
- (j) the day after Christmas Day
- (k) days declared by the Minister: (i) observance of the Sovereign's birthday; (ii) Show Day

s9(2) lets Regulations add or substitute days. **The Act text contains no weekend-substitution rule.** Whether a later compilation or Regulations add one was **not checked → UNVERIFIED**.

**Other sources:**
- In 2016 the Administrator confirmed that Anniversary (Bounty) Day, Foundation Day, Show Day and Thanksgiving Day continue. Seen only as a search snippet; the page timed out: https://www.infrastructure.gov.au/territories-regions-cities/territories/norfolk_island/administrator/media/2016/ni-a-mr-201626
- King's Birthday 2026 = Monday 15 June 2026. By tradition it is held "on the Monday after Bounty Day": https://www.norfolkonlinenews.com/article/kings-birthday-weekend-2026 (the article's "15 June 2025" is a typo; the article is dated June 2026).
- Secondary aggregator, read in a browser: https://www.timeanddate.com/holidays/norfolk-island/2026 and https://www.timeanddate.com/holidays/norfolk-island/2027.

| Holiday | 2026 | 2027 | Status |
|---|---|---|---|
| New Year's Day | Thu 1 Jan | Fri 1 Jan | Act |
| Australia Day | Mon 26 Jan | Tue 26 Jan | Act |
| Foundation Day | Fri 6 Mar | **Sat** 6 Mar | Act. 2027 substitute: none listed by timeanddate (UNVERIFIED) |
| Good Friday | 3 Apr | 26 Mar | Act; Easter dates computed |
| Easter Monday | 6 Apr | 29 Mar | Act |
| ANZAC Day | **Sat** 25 Apr | **Sun** 25 Apr | Act. timeanddate omits it for 2026 and lists no Monday substitute for either year (UNVERIFIED) |
| Anniversary (Bounty) Day | Mon 8 Jun | Tue 8 Jun | Act |
| King's Birthday (Sovereign's Birthday) | Mon 15 Jun | Mon 14 Jun | 2026 verified (NOL). 2027 per timeanddate and the "Monday after Bounty Day" tradition: UNVERIFIED (Minister declares) |
| Show Day (Norfolk Island Agricultural Show) | Mon 12 Oct | Mon 11 Oct | timeanddate only: UNVERIFIED (Minister declares) |
| Thanksgiving Day | Wed 25 Nov | Wed 24 Nov | Act (last Wednesday of November) |
| Christmas Day | Fri 25 Dec | **Sat** 25 Dec | Act |
| Boxing Day | **Sat** 26 Dec | **Sun** 26 Dec | Act |
| Observed substitutes | Mon 28 Dec (Boxing Day observed) | Mon 27 Dec (Christmas observed), Tue 28 Dec (Boxing observed) | timeanddate only: UNVERIFIED |

Not Norfolk Island public holidays per the Act: Labour Day, Melbourne Cup, etc. These are mainland-only and are not listed in s9 [INFERENCE from absence].

Machine-readable list: `server/seed-data/holidays.csv`.

## Timezone
- IANA id: **Pacific/Norfolk**. Base offset UTC+11:00 since July 2019. Uses rule set `AN` (the NSW daylight-saving rules) from 2019, i.e. UTC+12 during daylight saving. Source: https://data.iana.org/time-zones/tzdb/australasia (Zone Pacific/Norfolk lines 661–667).
- Use the tz database for conversions; do not hard-code the offset.

## Currency
- Australian dollar (AUD). NIRC fees are quoted in "$". The fees schedule mentions federal (Cth) legislation and Australian GST/ABN context.
- Fees schedule: https://www.nirc.gov.au/files/assets/public/v/1/corporate-and-finance/documents/fy-2026-27-nirc-fees-and-charges-report.pdf
- That the currency is AUD is an [INFERENCE]: Norfolk Island is an Australian external territory and no other currency appears in the sources.

## Address format
Postcode **2899**. Locality "Norfolk Island" (often "NORFOLK ISLAND 2899"). The town centre is **Burnt Pine**. Property is identified by **Portion number**, optionally with Lot and Section numbers.

| Example | Source |
|---|---|
| "Bicentennial Complex, 39 Taylors Rd, Burnt Pine, Norfolk Island" | https://www.nirc.gov.au/Customer-Service/Customer-care-team |
| "PO Box 95, Norfolk Island 2899" | same page |
| "Works Depot, New Cascade Road, Norfolk Island 2899" | https://www.nirc.gov.au/Infrastructure/Infrastructure-Services/Works-depot |
| "Taylors Road, Burnt Pine 2899" | https://www.nirc.gov.au/Customer-Service/Rawson-hall |
| "Location: Portion 52b1, 136a Taylors Road" | Gazette No. 33, 22 Aug 2025, https://www.nirc.gov.au/files/assets/public/v/1/your-council/documents/nirc-gazettes/2025/2025-08-22-gazette-no-33.pdf |
| Form property fields "Portion No. / Lot No. / Section No. / Land Area" | DA/BA form, https://www.nirc.gov.au/files/assets/public/v/1/planning-development/documents/application_for_da_ba_approval_form_05_09_24.pdf |

**Phone format:**
- `+6723 2xxxx`, e.g. +6723 22001.
- Local free call to Customer Care: `0100`.
- Staff extensions on forms are 5 digits, e.g. 22078.

**Email domain:** `@nirc.gov.nf`. Web domain: nirc.gov.au. Older forms cite www.norfolkisland.gov.nf.

**Council registration number:** ABN 60 103 855 713 (site footer).

## Branding notes
- **Do not reuse the official NIRC logo or crest.** Use a neutral, clearly "demo" identity.
- The site runs on OpenCities (Granicus) with meta `theme-color #FFFFFF`. The OpenGraph share image is `/files/ocwebsite/Public/HeroImage/share-img.png`. This is observed metadata only; do not copy the assets.
- The official name is "Norfolk Island Regional Council" (NIRC). Department and team names used on the site:
  - Customer Care / Customer Care Team
  - Planning & Development (also "Planning & Building Office")
  - Works Depot
  - Records Team; IT Team
- Website footer: "NIRC welcomes your feedback".

## Q2 Operational Plan update (as at 31 Dec 2025)
Source: https://www.nirc.gov.au/files/assets/public/v/5/your-council/documents/delivery-program/q2-update-operational-plan-as-at-31-december-2025.pdf. "Norfolk Island Regional Council – 2025-26 Operational Plan Q2 Report", 53 pp.

**3.6 Customer Service** (p.18):

| Ref | Action | Measure / Target | Responsibility |
|---|---|---|---|
| 3.6.1 | Continue developing online customer service forms as part of updating the website | % of customer forms available as online fillable forms / at least 20% | Customer Care Team; IT Team |
| 3.6.2 | Annual review of the Customer Service Charter | by 31 Aug 2025 | Customer Care Team |
| 3.6.3 | Customer service survey | by 31 Dec 2025 | Customer Care Team |
| 3.6.4 | Review complaints policy and procedure | by 30 Jun 2026 | Customer Care Team |
| 3.6.5 | Review and re-establish a Customer Requests Management System for Council and the community | CRM re-established and in use / by 31 Aug 2025 | Customer Care Team; IT Team |
| 3.6.6 | Review of Customer Services | by 30 Jun 2026 | Manager Customer Care |

Q2 updates:
- **3.6.2**: the Charter was renewed with no changes.
- **3.6.3**: the survey is being prepared.
- **3.6.5**: "Currently looking at alternative programs to Civica CRM. The Customer Care team have had a demonstration with iConciergeCRM by Big Technology … well received … user-friendly for both staff and customers and capable of being tailored."

**5.10.5** (p.42), "Implement an online system for booking forms as part of Council's Digital First Strategy":
- Measure: % of forms available as online forms. Target: 10%. Responsibility: Records Team.
- Q2 update:
  - The OpenCities "Open forms" module proved inefficient. It would need possibly hundreds of hours to set up over 150 forms. It has limited scope, no document-collection integration, and appears unstable. Sections lack a way to manage or view the data. Support is narrow.
  - Recommendation: "research a system that allows the end user to book and manage all key customer requests through a dedicated app, which the Council administers", covering a wide scope of forms and requests, with a strong supplier-support component.
- Related: Council uses Content Manager (EDRMS) and Civica Altitude (p.42).
