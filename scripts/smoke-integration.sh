#!/usr/bin/env bash
# End-to-end cross-slice checks through curl. Requires cargo, npm, curl and python3.
# SMOKE_DATA_DIR keeps a database/logs for inspection; otherwise everything is temporary.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${SMOKE_DATA_DIR:-}" ]]; then DATA_DIR="$SMOKE_DATA_DIR"; KEEP=true; mkdir -p "$DATA_DIR"; else DATA_DIR="$(mktemp -d /tmp/nsh-int.XXXXXX)"; KEEP=false; fi
PORT="${SMOKE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')}"
export DATA_DIR PORT DEMO_MODE=true COOKIE_SECURE=false DEMO_RESET_HOURS=0
export WEB_DIST="$ROOT/web/dist" PUBLIC_BASE_URL="http://127.0.0.1:$PORT" INTERNAL_BASE_URL="http://127.0.0.1:$PORT"
export SEED_DATA_DIR="$ROOT/server/seed-data" RUST_LOG=warn
export SMOKE_CLOCK_FILE="$DATA_DIR/clock-offset"
printf '0\n' >"$SMOKE_CLOCK_FILE"
PID=""
cleanup() { if [[ -n "$PID" ]]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi; if [[ "$KEEP" == false ]]; then rm -rf "$DATA_DIR"; fi; }
trap cleanup EXIT
(cd "$ROOT/web" && npm run build) >"$DATA_DIR/web-build.log" 2>&1
(cd "$ROOT/server" && cargo run --quiet -- seed-demo) >"$DATA_DIR/seed.log" 2>&1 || { cat "$DATA_DIR/seed.log"; exit 1; }
seed_counts() {
  python3 - <<'COUNTS'
import json, os, sqlite3
with sqlite3.connect(os.environ['DATA_DIR']+'/servicehub.db') as db:
    names=[n for (n,) in db.execute("SELECT name FROM pragma_table_list WHERE schema=? AND type IN (?,?) AND name NOT LIKE ?",('main','table','virtual','sqlite_%'))]
    print(json.dumps({n:db.execute('SELECT COUNT(*) FROM "'+n+'"').fetchone()[0] for n in names},sort_keys=True))
COUNTS
}
# The CLI resets twice; compare all base table counts rather than assuming it is repeatable.
seed_counts >"$DATA_DIR/seed-counts-first.json"
(cd "$ROOT/server" && cargo run --quiet -- seed-demo) >>"$DATA_DIR/seed.log" 2>&1 || { cat "$DATA_DIR/seed.log"; exit 1; }
seed_counts >"$DATA_DIR/seed-counts-second.json"
cmp "$DATA_DIR/seed-counts-first.json" "$DATA_DIR/seed-counts-second.json"
(cd "$ROOT/server" && cargo build --quiet --example smoke-server) >"$DATA_DIR/server-build.log" 2>&1 || { cat "$DATA_DIR/server-build.log"; exit 1; }
(cd "$ROOT/server" && exec target/debug/examples/smoke-server) >"$DATA_DIR/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 100); do
  if curl -fsS "$PUBLIC_BASE_URL/api/health" >/dev/null 2>&1; then break; fi
  kill -0 "$PID" 2>/dev/null || { cat "$DATA_DIR/server.log"; exit 1; }
  sleep 0.1
done
python3 -u - <<'PY'
import collections, concurrent.futures, csv as csv_module, datetime, html.parser, json, os, pathlib, sqlite3, subprocess, threading, time, uuid
from zoneinfo import ZoneInfo
base=os.environ['PUBLIC_BASE_URL']; data=pathlib.Path(os.environ['DATA_DIR'])
tokens={}; identities={}
def req(persona, method, path, body=None, status=200, fields=None, idempotency=None, binary=False):
    jar=str(data/(persona+'.cookies')); out=data/('response-'+str(uuid.uuid4()))
    cmd=['curl','--silent','--show-error','--max-time','20','-b',jar,'-c',jar,'-X',method,'-o',str(out),'-w','%{http_code}',base+path]
    if method!='GET': cmd+=['-H','X-CSRF-Token: '+tokens.get(persona,'')]
    if idempotency: cmd+=['-H','Idempotency-Key: '+idempotency]
    if fields:
        for k,v in fields: cmd+=['-F',k+'='+str(v)]
    elif method!='GET': cmd+=['-H','Content-Type: application/json','--data',json.dumps(body or {})]
    actual=int(subprocess.check_output(cmd,text=True)); raw=out.read_bytes(); out.unlink()
    if actual not in (status if isinstance(status,tuple) else (status,)): raise AssertionError(f'{persona}: {method} {path}: expected {status}, got {actual}: {raw.decode(errors="replace")}')
    if binary: return raw
    return json.loads(raw) if raw else None

def login(p):
    if p in tokens: return
    tokens[p]=req(p,'GET','/api/me')['csrf_token']
    me=req(p,'POST','/api/demo/login',{'persona':p}); tokens[p]=me['csrf_token']
    if me['mfa_required']:
        codes=req(p,'GET','/api/demo/authenticator'); code=next(x['code'] for x in codes if x['persona']==p)
        me=req(p,'POST','/api/auth/totp',{'code':code}); tokens[p]=me['csrf_token']
    identities[p]=me['user']['id']

def detail(p,c): return req(p,'GET',f'/api/cases/{c}')
def revision(p,c): return detail(p,c)['case']['revision']
def action(p,c,a,reason='Integration smoke check',status=200):
    return req(p,'POST',f'/api/cases/{c}/actions/{a}',{'expected_revision':revision(p,c),'reason':reason},status=status)

# A real, small PDF fixture; no external PDF tools are needed by the smoke script.
objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>',b'<< /Length 0 >>\nstream\n\nendstream']
pdf=b'%PDF-1.4\n'; offsets=[0]
for i,obj in enumerate(objects,1): offsets.append(len(pdf)); pdf+=f'{i} 0 obj\n'.encode()+obj+b'\nendobj\n'
xref=len(pdf); pdf+=b'xref\n0 5\n0000000000 65535 f \n'+b''.join(f'{o:010} 00000 n \n'.encode() for o in offsets[1:])+f'trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
fixture=data/'drawing.pdf'; fixture.write_bytes(pdf)

def submit(slug, extra=None, org=None, persona='alexey'):
    login(persona)
    definition=req(persona,'GET','/api/public/services/'+slug)['definition']; answers={}
    for f in definition['fields']:
        condition=f.get('show_if')
        if condition and answers.get(condition['field'])!=condition['equals']: continue
        if not f.get('required'): continue
        kind=f['type']; key=f['key']
        if kind=='checkbox': value=True
        elif kind=='number': value=100
        elif kind=='select': value=f['options'][0]['value']
        elif kind=='multiselect': value=[f['options'][0]['value']]
        elif kind=='location': value={'lat':-29.04,'lng':167.95,'description':'Integration smoke: Taylors Road pothole'}
        elif kind=='date': value='2026-11-20'
        elif kind=='email': value='alexey@example.invalid'
        else: value={'applicant_name':('Ben Carter' if persona=='ben' else 'Alexey Turner'),'postal_address':'Fictional 44 Taylors Road','property_ref':'Portion DEMO-44, Taylors Road','complaint':'Feedback about Olga; private integration smoke'}.get(key,'Integration smoke fixture')
        answers[key]=value
    answers.update(extra or {})
    c=req(persona,'POST',f'/api/services/{slug}/drafts',{'applicant_org_id':org} if org else {})['id']
    req(persona,'PUT',f'/api/cases/{c}/draft',{'answers':answers})
    uploads={}
    for d in definition['documents']:
        if d.get('required'):
            uploads[d['key']]=req(persona,'POST',f'/api/cases/{c}/documents',fields=[('file','@'+str(fixture)),('requirement_key',d['key']),('title',d['label'])])
    key=str(uuid.uuid4()); submitted=req(persona,'POST',f'/api/cases/{c}/submit',idempotency=key)
    assert req(persona,'POST',f'/api/cases/{c}/submit',idempotency=key)==submitted
    return c,uploads

start_day=datetime.datetime.now(ZoneInfo('Pacific/Norfolk')).date()
def local_day(day): return start_day+datetime.timedelta(days=day)
def instant(day,clock):
    value=datetime.datetime.combine(local_day(day),datetime.time.fromisoformat(clock),ZoneInfo('Pacific/Norfolk'))
    return value.astimezone(datetime.timezone.utc).isoformat().replace('+00:00','Z')
def hall_slot(unit,day):
    return {'event_name':'Fictional family celebration','slot':{'unit_code':unit,'start_at':instant(day,'10:00'),'end_at':instant(day,'16:00'),'attendees':40},'alcohol':'no'}
def money(p,c): return req(p,'GET',f'/api/cases/{c}/money')
def until(predicate,label,seconds=30):
    deadline=time.monotonic()+seconds
    while time.monotonic()<deadline:
        if predicate(): return
        time.sleep(.15)
    raise AssertionError('Timed out waiting for '+label)
def sql(query,params=()):
    with sqlite3.connect(f'file:{data}/servicehub.db?mode=ro',uri=True) as db:
        return db.execute(query,params).fetchall()
def travel(days):
    tmp=data/'next-clock'; tmp.write_text(str(days*86400)); tmp.replace(os.environ['SMOKE_CLOCK_FILE'])
    personas=list(tokens); tokens.clear()
    for p in personas: login(p)
class Forms(html.parser.HTMLParser):
    def __init__(self): super().__init__(); self.actions=[]
    def handle_starttag(self,tag,attrs):
        attrs=dict(attrs)
        if tag=='form' and attrs.get('method','').lower()=='post': self.actions.append(attrs['action'])
def pay_invoice(p,c,duplicate=False):
    invoice=next(i for i in money(p,c)['invoices'] if i['kind']=='invoice' and i['outstanding_cents']>0)
    checkout=req(p,'POST',f'/api/cases/{c}/checkout',{'invoice_id':invoice['id']})['checkout_url']
    jar=str(data/(p+'.cookies'))
    page=subprocess.check_output(['curl','-fsSL','--max-time','20','-b',jar,checkout],text=True)
    forms=Forms(); forms.feed(page)
    route=next(a for a in forms.actions if a.endswith('/duplicate' if duplicate else '/success'))
    before=len(money(p,c)['payments'])
    returned=subprocess.check_output(['curl','-fsSL','--max-time','20','-b',jar,'--data','',base+route],text=True)
    assert '<html' in returned.lower()
    assert len(money(p,c)['payments'])==before, 'Redirect must not confirm payment'
    until(lambda: money(p,c)['summary']['settled'],'payment webhook')
    assert len(money(p,c)['payments'])==before+1
    if duplicate:
        session=checkout.rsplit('/',1)[1]
        until(lambda: sql("SELECT attempts FROM mock_pay_webhook_attempts WHERE json_extract(payload_json,'$.session_id')=?",(session,))[0][0]>=2,'duplicate webhook delivery')
        assert len(money(p,c)['payments'])==before+1
        assert sql('SELECT COUNT(*) FROM payments WHERE external_id=(SELECT payment_id FROM mock_pay_sessions WHERE session_id=?)',(session,))[0][0]==1
def complete_task(c,kind):
    task=next(t for t in req('olga','GET',f'/api/cases/{c}/tasks') if t['kind']==kind)
    tid=task['id']
    assert task['assigned_to']==identities['jake']
    def update(kind,body):
        t=req('jake','GET',f'/api/field/tasks/{tid}')
        return req('jake','POST',f'/api/field/tasks/{tid}/updates',{'client_command_id':str(uuid.uuid4()),'expected_revision':t['revision'],'kind':kind,'body':body})
    for item in task['checklist']: update('checklist',json.dumps({'key':item['key'],'done':True}))
    update('result','Fictional work complete. Extra cleaning recorded at inspection.'); update('status','done')
def issue_decision(c,kind,outcome='approved'):
    login('helen')
    template=next(t for t in req('priya','GET','/api/decision-templates') if t['decision_type']==kind)
    did=req('priya','POST',f'/api/cases/{c}/decisions',{'decision_type':kind,'outcome':outcome,'reasons':'Fictional specialist assessment completed.','conditions':'Demo only.','template_id':template['id'],'expected_revision':revision('priya',c)})['id']
    for command in ['submit','issue']:
        approver = 'helen' if command == 'issue' else 'priya'
        req(approver,'POST',f'/api/cases/{c}/decisions/{did}/{command}',{'expected_revision':revision(approver,c)})
def trial_balance(c=None):
    journal=req('tom','GET','/api/finance/ledger'+(f'?case_id={c}' if c else ''))
    assert journal['entries'] and sum(r['balance_cents'] for r in journal['trial_balance'])==0
    totals=collections.defaultdict(int)
    for row in journal['entries']: totals[row['id']]+=row['debit_cents']-row['credit_cents']
    assert all(value==0 for value in totals.values())
def closed_delivery(c):
    until(lambda: any(x['kind']=='record.case_closed' and x['status']=='accepted' for x in req('olga','GET',f'/api/cases/{c}/integrations')),'closed case records delivery')

try:
    print('ok   seed-demo twice: base table counts identical')
    login('olga'); login('tom'); login('jake'); login('ben')
    rates={'EQUIP_EXCAVATOR_HOUR':('Bobcat',13500,'EXCAVATOR'),
           'EQUIP_BACKHOE_HOUR':('Volvo Loader',23000,'BACKHOE'),
           'EQUIP_TIPPER_HOUR':('Hino Truck',11000,'TIPPER'),
           'EQUIP_ROLLER_HOUR':('Cat Steel Drum Roller 8T',21100,'ROLLER')}
    prices=req('tom','GET','/api/admin/prices')['items']
    for code,(name,rate,_) in rates.items():
        item=next(x for x in prices if x['code']==code)
        assert name in item['name'] and item['versions'][0]['amount_cents']==rate
    print('ok   Tom can manage prices; all four fleet codes/names/rates agree')

    # Create isolated payment fixtures and bind the sample CSV to their allocated case numbers.
    first,_=submit('rawson-hall-hire',hall_slot('rawson-main',10))
    second,_=submit('rawson-hall-hire',hall_slot('rawson-main',11),persona='ben')
    first_number=detail('olga',first)['case']['number']
    second_number=detail('olga',second)['case']['number']
    action('olga',first,'advance'); action('olga',second,'advance')
    csv=pathlib.Path(os.environ['SEED_DATA_DIR'],'statements/demo-statement.csv').read_text()
    sample_rows=list(csv_module.DictReader(csv.splitlines()))
    csv=csv.replace(sample_rows[0]['reference'],first_number).replace(sample_rows[1]['reference'],second_number)
    report=req('tom','POST','/api/finance/statements',{'filename':'demo-statement.csv','csv':csv})
    assert collections.Counter(r['status'] for r in report['rows'])=={'matched':1,'unmatched':2,'duplicate':1}, report
    assert detail('alexey',first)['case']['current_step']=='confirm'
    partial=next(r for r in report['rows'] if r['bank_txn_id']=='DEMO-BANK-002')
    assert partial['suggested_case_id']==second
    req('tom','POST',f'/api/finance/statement-rows/{partial["id"]}/match',{'case_id':second,'expected_revision':revision('tom',second)},status=204)
    assert money('ben',second)['summary']['outstanding_cents']==31500
    assert detail('ben',second)['case']['current_step']=='payment'
    req('tom','POST','/api/finance/statements',{'filename':'renamed-same-file.csv','csv':csv},status=409)
    assert len(req('tom','GET','/api/finance/unmatched')['rows'])==1
    trial_balance()
    print('ok   seeded CSV: 1 matched / 2 unmatched / 1 duplicate; partial matched manually; unclear transfer stays in suspense; same file → 409')

    cert,_=submit('planning-certificate',{'sections':'Zoning and heritage'})
    action('olga',cert,'advance')
    assert detail('alexey',cert)['case']['current_step']=='payment'
    action('tom',cert,'advance',status=409)
    pay_invoice('alexey',cert)
    assert detail('alexey',cert)['case']['current_step']=='preparation'
    login('priya'); action('priya',cert,'advance')
    issue_decision(cert,'planning_certificate')
    issued=req('alexey','GET',f'/api/cases/{cert}/decisions')['items'][0]
    assert req('alexey','GET',f'/api/document-versions/{issued["output_document_version_id"]}/download',binary=True).startswith(b'%PDF-')
    assert detail('alexey',cert)['case']['status']=='completed'
    closed_delivery(cert)
    until(lambda: {'payment.receipt','document.decision','record.case_closed'}.issubset({x['kind'] for x in req('olga','GET',f'/api/cases/{cert}/integrations') if x['status']=='accepted'}),'certificate/receipt/closure deliveries')
    print('ok   planning certificate → unpaid advance blocked → DemoPay → specialist → issued certificate PDF → completed → records delivered')
    refused,_=submit('planning-certificate',{'sections':'Fictional refusal check'})
    action('olga',refused,'advance'); pay_invoice('alexey',refused); action('priya',refused,'advance')
    issue_decision(refused,'planning_certificate',outcome='refused')
    assert detail('alexey',refused)['case']['status']=='refused'
    print('ok   paid certificate with specialist refusal closes as refused, with issued decision retained')

    hall,_=submit('rawson-hall-hire',hall_slot('rawson-whole',21))
    notifications=req('alexey','GET','/api/notifications')['items']
    assert any(n['case_id']==hall and 'Booking not confirmed yet' in n['body'] for n in notifications)
    assert any('not yet confirmed' in e['summary'] for e in detail('alexey',hall)['timeline'])
    action('olga',hall,'advance')
    charges=money('alexey',hall); invoice=next(i for i in charges['invoices'] if i['kind']=='invoice')
    assert [(l['kind'],l['amount_cents']) for l in invoice['lines']]==[('fee',15500),('deposit',25000)]
    pay_invoice('alexey',hall,duplicate=True)
    assert detail('alexey',hall)['case']['current_step']=='confirm'
    assert len(money('alexey',hall)['payments'])==1
    booking=req('olga','GET',f'/api/cases/{hall}/booking')['booking']
    req('olga','POST',f'/api/cases/{hall}/booking/confirm',{'expected_revision':booking['revision']})
    original=req('alexey','GET',f'/api/cases/{hall}/booking')['booking']
    old_pdf=req('alexey','GET',f'/api/cases/{hall}/booking/confirmation/{original["confirmation_version_id"]}',binary=True)
    assert old_pdf.startswith(b'%PDF-') and detail('alexey',hall)['case']['current_step']=='prep'
    move=hall_slot('rawson-main',22)['slot']
    move={'unit_code':move['unit_code'],'start':move['start_at'],'end':move['end_at'],
          'reason':'Family celebration moved to the next day; Main Hall is sufficient.', 'expected_revision':original['revision']}
    preview=req('olga','POST',f'/api/cases/{hall}/booking/reschedule/preview',move)
    assert preview['available'] and preview['old_lines'][0]['amount_cents']==15500 and preview['new_lines'][0]['amount_cents']==11500
    assert req('olga','POST',f'/api/cases/{hall}/booking/reschedule',move)['fees_changed']
    history=req('alexey','GET',f'/api/cases/{hall}/booking')
    assert any(h['revision']==original['revision'] and h['start_at']==original['start_at'] for h in history['history'])
    assert req('alexey','GET',f'/api/cases/{hall}/booking/confirmation/{original["confirmation_version_id"]}',binary=True)==old_pdf
    assert req('alexey','GET',f'/api/cases/{hall}/booking/confirmation/{history["booking"]["confirmation_version_id"]}',binary=True).startswith(b'%PDF-')
    charges=money('alexey',hall)
    assert charges['summary']['settled'] and charges['customer_credit_cents']==4000
    assert len([l for i in charges['invoices'] if i['kind']=='invoice' for l in i['lines'] if l['kind']=='deposit'])==1
    assert any(i['kind']=='credit_note' for i in charges['invoices'])
    complete_task(hall,'venue_prep')
    assert detail('alexey',hall)['case']['current_step']=='inspect'
    print('ok   Alexey: unconfirmed submission → fee + separate bond → duplicate DemoPay delivery produces one payment → Olga confirmation PDF → reschedule/history/fee credit → Jake prep')

    whole,_=submit('rawson-hall-hire',hall_slot('rawson-whole',23))
    main,_=submit('rawson-hall-hire',hall_slot('rawson-main',23),persona='ben')
    for c,p in [(whole,'alexey'),(main,'ben')]:
        action('olga',c,'advance'); pay_invoice(p,c)
    barrier=threading.Barrier(2)
    def confirm(c):
        booking=req('olga','GET',f'/api/cases/{c}/booking')['booking']
        barrier.wait(timeout=10)
        return req('olga','POST',f'/api/cases/{c}/booking/confirm',{'expected_revision':booking['revision']},status=(200,409))
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results=list(pool.map(confirm,[whole,main]))
    statuses=[req('olga','GET',f'/api/cases/{c}/booking')['booking']['status'] for c in [whole,main]]
    assert sorted(statuses)==['confirmed','requested'] and sum('error' in r and r['error']['code']=='conflict' for r in results)==1
    print('ok   two residents, whole hall/Main Hall overlap, both paid: parallel confirmation → exactly one confirmed, one 409')

    equipment=[]
    for index,(code,(name,rate,resource)) in enumerate(rates.items()):
        day=24+index
        e,_=submit('equipment-hire',{'request':{'description':name,'requested_hours':4,'preferred_date':local_day(day).isoformat(),'site_text':'Fictional depot work site'}})
        estimate=next(i for i in money('alexey',e)['invoices'] if i['kind']=='estimate')
        assert estimate['total_cents']==4*rate and money('alexey',e)['summary']['outstanding_cents']==0
        action('olga',e,'advance')
        req('olga','POST',f'/api/cases/{e}/equipment/schedule',{'resource_code':resource,'operator_user_id':identities['jake'],'start':instant(day,'07:30'),'end':instant(day,'13:30'),'expected_revision':revision('olga',e)})
        assert detail('olga',e)['case']['current_step']=='job'
        equipment.append((e,day,rate))

    # Only the test harness clock moves. No workflow/payment/booking rows are written by the script.
    travel(29)
    complete_task(hall,'venue_inspection')
    assert detail('alexey',hall)['case']['current_step']=='bond'
    charges=money('alexey',hall); assert charges['deposit_ready']
    bond=next(l for i in charges['invoices'] if i['kind']=='invoice' for l in i['lines'] if l['kind']=='deposit')
    decision={'invoice_line_id':bond['id'],'refund_cents':20000,'retain_items':[{'label':'Extra cleaning','cents':5000}],
              'reason':'Extra cleaning after the family celebration.','expected_revision':revision('tom',hall)}
    key=str(uuid.uuid4())
    saved=req('tom','POST',f'/api/cases/{hall}/deposit-decision',decision,idempotency=key)
    assert req('tom','POST',f'/api/cases/{hall}/deposit-decision',decision,idempotency=key)==saved
    charges=money('alexey',hall)
    assert charges['refunds'][0]['status']=='processing' and charges['refunds'][0]['method']=='provider'
    assert charges['deposit_decisions'][0]['retain_items']==[{'label':'Extra cleaning','cents':5000}]
    assert detail('alexey',hall)['case']['status']!='completed'
    action('tom',hall,'advance',status=409)
    until(lambda: money('alexey',hall)['refunds'][0]['status']=='completed','refund webhook')
    assert detail('alexey',hall)['case']['status']=='completed'
    assert money('alexey',hall)['summary']['deposits_held_cents']==0
    trial_balance(hall)
    closed_delivery(hall)
    print('ok   Alexey: Jake inspection → Tom itemises $50 Extra cleaning / $200 refund → processing cannot close → DemoPay refund webhook → completed → records delivered; case ledger zero-sum')

    for e,day,rate in equipment:
        task=next(t for t in req('olga','GET',f'/api/cases/{e}/tasks') if t['kind']=='equipment_job')
        usage=req('jake','POST',f'/api/field/tasks/{task["id"]}/usage',{'client_command_id':str(uuid.uuid4()),'started_at':instant(day,'07:30'),'ended_at':instant(day,'13:30'),'downtime_minutes':30,'expenses_cents':1234,'expenses_note':'Agreed transport expenses'})
        assert usage['billable_minutes']==330
        complete_task(e,'equipment_job')
        assert detail('olga',e)['case']['current_step']=='usage'
        approved=req('tom','POST',f'/api/cases/{e}/equipment/usage/{usage["usage_id"]}/approve')
        assert req('tom','POST',f'/api/cases/{e}/equipment/usage/{usage["usage_id"]}/approve')['invoice_id']==approved['invoice_id']
        assert detail('tom',e)['case']['current_step']=='payment'
        invoices=[i for i in money('alexey',e)['invoices'] if i['kind']=='invoice']
        assert len(invoices)==1 and invoices[0]['id']==approved['invoice_id']
        final=invoices[0]
        assert final['total_cents']==(330*rate+30)//60+1234
        assert final['lines'][0]['quantity_minutes']==330
        assert all(text in final['basis_note'] for text in ['Requested 4 h','actual 6 h 0 min','billable 5 h 30 min','07:30','13:30','30 min downtime'])
        assert req('alexey','GET',f'/api/document-versions/{final["document_version_id"]}/download',binary=True).startswith(b'%PDF-')
        pay_invoice('alexey',e)
        assert detail('alexey',e)['case']['status']=='completed'
        closed_delivery(e)
    trial_balance()
    print('ok   all four plant rates: estimate 4 h → actual 07:30–13:30 / 30 min downtime → one final invoice 330 min × rate / 60 + $12.34 expenses → PDF/basis → paid → completed; global ledger zero-sum')

    building,docs=submit('development-application')
    action('olga',building,'advance'); login('priya')
    drawing=docs['floor_plans']; vid=drawing['version_id']
    comment=req('priya','POST',f'/api/document-versions/{vid}/comments',{'expected_revision':revision('priya',building),'body':'Please replace drawing A-101 with the corrected dimensions.','visibility':'applicant','request_new_version':True})['id']
    d=detail('alexey',building); assert d['required_action'] and any(x['status']=='paused' for x in d['deadlines'])
    # A text acknowledgement cannot satisfy a drawing replacement request.
    req('alexey','POST',f'/api/cases/{building}/messages',{'body':'I will upload the corrected drawing.'})
    assert detail('alexey',building)['required_action']
    v2=req('alexey','POST',f'/api/documents/{drawing["id"]}/versions',fields=[('file','@'+str(fixture)),('resolves_comment_ids',json.dumps([comment])),('note','Corrected drawing A-101')])
    d=detail('alexey',building); assert d['required_action'] is None and d['case']['status']=='in_progress'
    assert all(x['status']!='paused' for x in d['deadlines'])
    assert req('alexey','GET',f'/api/document-versions/{vid}/download',binary=True)==pdf
    action('priya',building,'advance'); action('priya',building,'skip','Exhibition is not required for this fictional test application.')
    login('helen')
    templates=req('priya','GET','/api/decision-templates')
    for decision_type in ['development_approval','building_approval']:
        template=next(t for t in templates if t['decision_type']==decision_type)
        did=req('priya','POST',f'/api/cases/{building}/decisions',{'decision_type':decision_type,'outcome':'approved','reasons':'Fictional demo assessment completed.','conditions':'Follow approved drawing A-101 v2.','template_id':template['id'],'evidence_version_ids':[v2['version_id']],'expected_revision':revision('priya',building)})['id']
        for command in ['submit','issue']:
            approver = 'helen' if command == 'issue' else 'priya'
            req(approver,'POST',f'/api/cases/{building}/decisions/{did}/{command}',{'expected_revision':revision(approver,building)})
    assert detail('alexey',building)['case']['status']=='completed'
    print('ok   building intake → replacement request → text reply keeps action → v2 clears action/resumes clock → two independently issued approvals → completed')

    approvals=req('alexey','GET','/api/my/issued-approvals')
    approval=next(a for a in approvals if a['decision_type']=='building_approval')
    project=req('alexey','GET',f'/api/building-projects/{approval["project_id"]}')
    modification,_=submit('modify-approval',{'original_approval':{'decision_id':approval['id']}})
    commencement,_=submit('building-commencement-notice',{'project_reference':project['reference']})
    completion,_=submit('building-completion-notice',{'project_reference':str(project['id'])})
    linked=req('alexey','GET',f'/api/building-projects/{project["id"]}')
    assert {building,modification,commencement,completion}.issubset({c['id'] for c in linked['cases']})
    print('ok   canonical original_approval/project_reference answers link modification and both notices to the building project')

    road,_=submit('road-issue'); action('olga',road,'advance'); login('jake')
    for task_kind in ['road_inspection','road_repair']:
        tasks=req('olga','GET',f'/api/cases/{road}/tasks'); task=next(t for t in tasks if t['kind']==task_kind); tid=task['id']
        def task_update(kind,body):
            t=req('jake','GET',f'/api/field/tasks/{tid}')
            return req('jake','POST',f'/api/field/tasks/{tid}/updates',{'client_command_id':str(uuid.uuid4()),'expected_revision':t['revision'],'kind':kind,'body':body})
        for item in task['checklist']: task_update('checklist',json.dumps({'key':item['key'],'done':True}))
        task_update('result','Inspected and repaired pothole for this fictional test.'); task_update('status','done')
    assert detail('olga',road)['case']['current_step']=='response'
    req('olga','POST',f'/api/cases/{road}/road-response',{'expected_revision':revision('olga',road),'body':'Our inspection is complete. The road issue has been recorded and no repair is required.'})
    assert detail('alexey',road)['case']['status']=='completed'
    for _ in range(100):
        deliveries=req('olga','GET',f'/api/cases/{road}/integrations')
        if any(x['status']=='accepted' for x in deliveries): break
        time.sleep(.1)
    else: raise AssertionError('road integration did not deliver: '+json.dumps(deliveries))
    print('ok   road report → Jake inspection done → Jake repair done → response letter → closed → integration delivered')

    complaint,_=submit('complaint'); login('ruth')
    req('ruth','POST',f'/api/cases/{complaint}/complaint/subjects',{'staff_user_ids':[identities['olga']],'expected_revision':revision('ruth',complaint)})
    req('olga','GET',f'/api/cases/{complaint}',status=404)
    req('ruth','POST',f'/api/cases/{complaint}/messages',{'body':'Sensitive internal feedback details about Olga','expected_revision':revision('ruth',complaint)})
    mailbox=req('ruth','GET','/api/demo/mailbox')
    # Queue rows are visible immediately, even before outbound delivery.
    assert all('Sensitive internal feedback' not in n['body'] and 'Feedback about Olga' not in n['body'] for n in mailbox)
    action('ruth',complaint,'advance'); action('ruth',complaint,'advance')
    req('ruth','POST',f'/api/cases/{complaint}/letters',{'letter_type':'complaint_response','title':'Feedback response','body':'Fictional complaint response.','expected_revision':revision('ruth',complaint)})
    review=req('alexey','POST',f'/api/cases/{complaint}/complaint/request-review',{'reason':'Please independently review the response.'})['id']
    login('helen'); original=detail('ruth',complaint); reviewed=detail('helen',review)
    owners=lambda d:[a['user_id'] for a in d['assignments'] if a['role']=='owner' and a['ended_at'] is None]
    assert owners(original)!=owners(reviewed) and reviewed['case']['current_step']=='triage'
    req('olga','GET',f'/api/cases/{review}',status=404)
    print('ok   confidential complaint submitted/assigned → Olga 404 → generic outbound → completed → independent review copies exclusions')
    login('ben'); org=req('ben','GET','/api/my/organisations')[0]['id']
    member=req('ben','POST',f'/api/my/organisations/{org}/invites',{'email':'alexey@demo.servicehub.invalid'})['id']
    mailbox=req('ben','GET','/api/demo/mailbox')
    invitation=next(n for n in reversed(mailbox) if '/my/invites/' in n['body'])
    token=invitation['body'].split('/my/invites/')[1].split()[0].rstrip('.')
    req('alexey','POST',f'/api/my/invites/{token}/accept')
    shared,_=submit('road-issue',org=org)
    upload=req('alexey','POST',f'/api/cases/{shared}/documents',fields=[('file','@'+str(fixture)),('title','Shared organisation drawing')])
    req('alexey','GET',f'/api/document-versions/{upload["version_id"]}/download',binary=True)
    req('ben','POST',f'/api/my/organisations/{org}/members/{member}/revoke')
    req('alexey','GET',f'/api/document-versions/{upload["version_id"]}/download',status=404)
    req('ben','GET',f'/api/document-versions/{upload["version_id"]}/download',binary=True)
    print('ok   revoked organisation member immediately receives 404 on the real document download; owner retains access')

    login('mark')
    csv_text='source_system,source_id,service_slug,applicant_name,applicant_email,property_ref,title,opened_on,closed_on,status,notes\nIntegration,INT-001,council-record-copy,Fictional Historical Applicant,historical@example.invalid,Portion DEMO-90,Historical record request,2000-01-01,2001-01-01,closed,Imported smoke fixture\n'
    preview=req('mark','POST','/api/admin/legacy-imports',{'filename':'integration.csv','csv':csv_text})
    assert preview['valid']==1 and preview['errors']==0
    imported=req('mark','POST',f'/api/admin/legacy-imports/{preview["id"]}/import',{'skip_possible_duplicates':True})
    assert req('mark','POST',f'/api/admin/legacy-imports/{preview["id"]}/import',{'skip_possible_duplicates':True})==imported
    historical=imported['rows'][0]['case_id']
    # Search uses the imported historical dates; compare the case metadata itself.
    h=detail('helen',historical)['case']
    assert h['submitted_at'].startswith('1999-12-31') and req('helen','GET',f'/api/records/search?number={h["number"]}')[0]['closed_at'].startswith('2001-01-01')
    req('helen','POST',f'/api/records/cases/{historical}/dispose',{'reason':'Historical demonstration retention period elapsed.','expected_revision':h['revision']})
    duplicate=req('mark','POST','/api/admin/legacy-imports',{'filename':'integration-repeat.csv','csv':csv_text})
    assert duplicate['duplicates']==1
    print('ok   historical legacy import preserves Norfolk dates; replay is idempotent; repeat upload detects exact duplicate; S5 disposal succeeds')
    trial_balance()
    req('mark','POST',f'/api/admin/users/{identities["alexey"]}/deactivate')
    assert req('alexey','GET','/api/me')['user'] is None
    req('alexey','GET',f'/api/cases/{building}',status=401)
    print('ok   S5 user deactivation immediately invalidates the resident session')
    print('PASS integration smoke')
except Exception:
    print((data/'server.log').read_text()[-4000:])
    raise
PY
