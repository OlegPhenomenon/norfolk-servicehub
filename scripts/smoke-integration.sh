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
PID=""
cleanup() { if [[ -n "$PID" ]]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi; if [[ "$KEEP" == false ]]; then rm -rf "$DATA_DIR"; fi; }
trap cleanup EXIT
(cd "$ROOT/web" && npm run build) >"$DATA_DIR/web-build.log" 2>&1
(cd "$ROOT/server" && cargo run --quiet -- seed-demo) >"$DATA_DIR/seed.log" 2>&1 || { cat "$DATA_DIR/seed.log"; exit 1; }
(cd "$ROOT/server" && exec cargo run --quiet -- serve) >"$DATA_DIR/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 100); do
  if curl -fsS "$PUBLIC_BASE_URL/api/health" >/dev/null 2>&1; then break; fi
  kill -0 "$PID" 2>/dev/null || { cat "$DATA_DIR/server.log"; exit 1; }
  sleep 0.1
done
python3 - <<'PY'
import json, os, pathlib, subprocess, time, uuid
base=os.environ['PUBLIC_BASE_URL']; data=pathlib.Path(os.environ['DATA_DIR'])
tokens={}; identities={}
def req(persona, method, path, body=None, status=200, fields=None, idempotency=None, binary=False):
    jar=str(data/(persona+'.cookies')); out=data/'response'
    cmd=['curl','--silent','--show-error','--max-time','20','-b',jar,'-c',jar,'-X',method,'-o',str(out),'-w','%{http_code}',base+path]
    if method!='GET': cmd+=['-H','X-CSRF-Token: '+tokens.get(persona,'')]
    if idempotency: cmd+=['-H','Idempotency-Key: '+idempotency]
    if fields:
        for k,v in fields: cmd+=['-F',k+'='+str(v)]
    elif method!='GET': cmd+=['-H','Content-Type: application/json','--data',json.dumps(body or {})]
    actual=int(subprocess.check_output(cmd,text=True)); raw=out.read_bytes()
    if actual!=status: raise AssertionError(f'{persona}: {method} {path}: expected {status}, got {actual}: {raw.decode(errors="replace")}')
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

def submit(slug, extra=None, org=None):
    login('alexey')
    definition=req('alexey','GET','/api/public/services/'+slug)['definition']; answers={}
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
        else: value={'applicant_name':'Alexey Turner','postal_address':'Fictional 44 Taylors Road','property_ref':'Portion DEMO-44, Taylors Road','complaint':'Feedback about Olga; private integration smoke'}.get(key,'Integration smoke fixture')
        answers[key]=value
    answers.update(extra or {})
    c=req('alexey','POST',f'/api/services/{slug}/drafts',{'applicant_org_id':org} if org else {})['id']
    req('alexey','PUT',f'/api/cases/{c}/draft',{'answers':answers})
    uploads={}
    for d in definition['documents']:
        if d.get('required'):
            uploads[d['key']]=req('alexey','POST',f'/api/cases/{c}/documents',fields=[('file','@'+str(fixture)),('requirement_key',d['key']),('title',d['label'])])
    key=str(uuid.uuid4()); submitted=req('alexey','POST',f'/api/cases/{c}/submit',idempotency=key)
    assert req('alexey','POST',f'/api/cases/{c}/submit',idempotency=key)==submitted
    return c,uploads

try:
    # Planning reaches the finance boundary; the stub's invoice error rolls intake back atomically.
    cert,_=submit('planning-certificate',{'sections':'Zoning and heritage'})
    login('olga'); before=revision('olga',cert)
    action('olga',cert,'advance',status=500)
    assert revision('olga',cert)==before and detail('alexey',cert)['case']['current_step']=='intake'
    print('ok   resident planning certificate submitted; intake advance attempted; atomic rollback at invoice stub')
    print('SKIP (finance pending): planning payment, certificate preparation and issuance')

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
    templates=req('priya','GET','/api/decision-templates')
    for decision_type in ['development_approval','building_approval']:
        template=next(t for t in templates if t['decision_type']==decision_type)
        did=req('priya','POST',f'/api/cases/{building}/decisions',{'decision_type':decision_type,'outcome':'approved','reasons':'Fictional demo assessment completed.','conditions':'Follow approved drawing A-101 v2.','template_id':template['id'],'evidence_version_ids':[v2['version_id']],'expected_revision':revision('priya',building)})['id']
        for command in ['submit','issue']:
            req('priya','POST',f'/api/cases/{building}/decisions/{did}/{command}',{'expected_revision':revision('priya',building)})
    assert detail('alexey',building)['case']['status']=='completed'
    print('ok   building intake → replacement request → text reply keeps action → v2 clears action/resumes clock → two Priya approvals → completed')

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
    req('mark','POST',f'/api/admin/users/{identities["alexey"]}/deactivate')
    assert req('alexey','GET','/api/me')['user'] is None
    req('alexey','GET',f'/api/cases/{building}',status=401)
    print('ok   S5 user deactivation immediately invalidates the resident session')
    print('PASS integration smoke')
except Exception:
    print((data/'server.log').read_text()[-4000:])
    raise
PY
