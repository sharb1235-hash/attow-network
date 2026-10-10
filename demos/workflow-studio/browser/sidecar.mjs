// Original fixture adapter. No arbitrary URL, script, credentials or live mode.
import http from 'node:http';
import {readFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {chromium} from 'playwright';
const require=createRequire(import.meta.url);
const FRAME=524288, SNAPSHOT=262144;
if(process.versions.node!=='24.21.0'||require('playwright/package.json').version!=='1.64.0') throw Error('unsupported runtime');
const revisions=JSON.parse(await readFile(new URL('./node_modules/playwright-core/browsers.json',import.meta.url),'utf8'));
if(revisions.browsers.find(x=>x.name==='chromium').revision!=='1248') throw Error('unsupported revision');
const html=await readFile(new URL('../app/index.html',import.meta.url));
const app=await readFile(new URL('../app/app.js',import.meta.url));
let swRequests=0, forbiddenRequests=0;
const fixture=http.createServer((req,res)=>{
  res.setHeader('Content-Security-Policy',"default-src 'self'; script-src 'self'; connect-src 'self'; worker-src 'none'; object-src 'none'");
  if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end(html);}
  else if(req.url==='/app.js'){res.setHeader('Content-Type','text/javascript');res.end(app);}
  else if(req.url==='/sw.js'){swRequests++;res.setHeader('Content-Type','text/javascript');res.end('self.addEventListener("fetch",()=>{});');}
  else {res.statusCode=404;res.end();}
});
const forbidden=http.createServer((_req,res)=>{forbiddenRequests++;res.end('forbidden');});
forbidden.on('upgrade',(_req,socket)=>{forbiddenRequests++;socket.destroy();});
await Promise.all([new Promise(resolve=>fixture.listen(0,'127.0.0.1',resolve)),new Promise(resolve=>forbidden.listen(0,'127.0.0.1',resolve))]);
const origin=`http://127.0.0.1:${fixture.address().port}`;
const denied=`http://127.0.0.1:${forbidden.address().port}`;
const browser=await chromium.launch({headless:true,args:['--disable-background-networking','--disable-component-update','--disable-sync','--no-first-run']});
if(browser.version()!=='156.0.8078.4') throw Error('unsupported executable');
let context,page;
async function fresh(){
  const previous=context;
  context=await browser.newContext({serviceWorkers:'block',acceptDownloads:false});
  context.setDefaultTimeout(5000);context.setDefaultNavigationTimeout(5000);
  await context.route('**/*',route=>{
    const url=new URL(route.request().url());
    if(url.origin===origin&&['/','/app.js','/sw.js'].includes(url.pathname)&&route.request().method()==='GET') return route.continue();
    return route.abort('blockedbyclient');
  });
  await context.routeWebSocket('**/*',socket=>socket.close());
  page=await context.newPage();await page.goto(origin);
  if(previous) await previous.close();
}
await fresh();
const plain=value=>value!==null&&typeof value==='object'&&!Array.isArray(value);
function exact(value,keys){if(!plain(value)||Object.keys(value).sort().join(',')!==[...keys].sort().join(',')) throw Error('invalid fields');}
function pair(key,value){if(typeof key!=='string'||!key.length||key.length>128||typeof value!=='string'||value.length>4096) throw Error('invalid storage value');}
function entries(values){if(!Array.isArray(values)||values.length>128) throw Error('storage limit');const keys=new Set();for(const item of values){if(!Array.isArray(item)||item.length!==2)throw Error('storage entry');pair(...item);if(keys.has(item[0]))throw Error('duplicate key');keys.add(item[0]);}}
async function indexed(action,key,value){
  return page.evaluate(async ({action,key,value})=>{
    const db=await new Promise((resolve,reject)=>{const request=indexedDB.open('attow-fixture',1);request.onupgradeneeded=()=>request.result.createObjectStore('values');request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(Error('open'));});
    try{return await new Promise((resolve,reject)=>{
      const transaction=db.transaction('values',action==='set'?'readwrite':'readonly');const store=transaction.objectStore('values');let result;
      transaction.oncomplete=()=>resolve(result??null);transaction.onerror=()=>reject(Error('transaction'));
      if(action==='set'){store.put(value,key);result=true;}
      else if(action==='get'){const request=store.get(key);request.onsuccess=()=>result=request.result??null;}
      else {result=[];const request=store.openCursor();request.onsuccess=()=>{const cursor=request.result;if(cursor){result.push([cursor.key,cursor.value]);cursor.continue();}};}
    });}finally{db.close();}
  },{action,key,value});
}
async function snapshot(){
  const cookies=await context.cookies(origin);
  const local=await page.evaluate(()=>Object.entries(localStorage));
  const session=await page.evaluate(()=>Object.entries(sessionStorage));
  const database=await indexed('all');
  const result={version:1,fixture:'attow-owned-v1',cookies,local,session,database};
  validateSnapshot(result);return result;
}
function validateSnapshot(value){
  exact(value,['version','fixture','cookies','local','session','database']);
  if(value.version!==1||value.fixture!=='attow-owned-v1'||Buffer.byteLength(JSON.stringify(value))>SNAPSHOT)throw Error('snapshot limit');
  entries(value.local);entries(value.session);entries(value.database);
  if(!Array.isArray(value.cookies)||value.cookies.length>128)throw Error('cookie limit');
  const names=new Set();
  for(const cookie of value.cookies){
    const keys=['name','value','domain','path','expires','httpOnly','secure','sameSite'];
    exact(cookie,keys);pair(cookie.name,cookie.value);
    if(names.has(cookie.name)||cookie.domain!=='127.0.0.1'||cookie.path!=='/'||cookie.expires!==-1||cookie.httpOnly!==false||cookie.secure!==false||cookie.sameSite!=='Lax')throw Error('unsupported cookie');
    names.add(cookie.name);
  }
}
async function restore(value){
  validateSnapshot(value);await fresh();
  await context.addCookies(value.cookies);
  await page.evaluate(({local,session})=>{for(const [key,value]of local)localStorage.setItem(key,value);for(const [key,value]of session)sessionStorage.setItem(key,value);},value);
  for(const [key,storedValue]of value.database)await indexed('set',key,storedValue);
}
async function method(name,args){
  if(name==='hello'){exact(args,[]);return {node:process.versions.node,playwright:'1.64.0',chromium:browser.version(),revision:'1248',mode:'OwnedEnvironment',credentials:false};}
  if(name==='snapshot'){exact(args,[]);return snapshot();}
  if(name==='restore'){exact(args,['snapshot']);await restore(args.snapshot);return {restored:true};}
  if(name==='storage_set'||name==='storage_get'){
    exact(args,name==='storage_set'?['kind','key','value']:['kind','key']);pair(args.key,name==='storage_set'?args.value:'');
    if(!['cookies','local','session','database'].includes(args.kind))throw Error('storage kind');
    if(name==='storage_set'){
      // Limit total storage before adding a new key.
      const before=await snapshot();const target=args.kind==='cookies'?before.cookies.map(c=>[c.name,c.value]):before[args.kind];
      if(target.length>=128&&!target.some(entry=>entry[0]===args.key))throw Error('storage limit');
      if(args.kind==='cookies')await context.addCookies([{name:args.key,value:args.value,domain:'127.0.0.1',path:'/',sameSite:'Lax'}]);
      else if(args.kind==='database')await indexed('set',args.key,args.value);
      else await page.evaluate(({kind,key,value})=>(kind==='local'?localStorage:sessionStorage).setItem(key,value),args);
      try{await snapshot();}catch(error){await restore(before);throw error;}
      return {stored:true};
    }
    if(args.kind==='cookies')return (await context.cookies(origin)).find(c=>c.name===args.key)?.value??null;
    if(args.kind==='database')return indexed('get',args.key);
    return page.evaluate(({kind,key})=>(kind==='local'?localStorage:sessionStorage).getItem(key),args);
  }
  if(name==='network_probe'){
    exact(args,[]);const initial=forbiddenRequests,initialSw=swRequests;
    const result=await page.evaluate(async denied=>{
      const fetch_blocked=await fetch(denied).then(()=>false,()=>true);
      const websocket_blocked=await new Promise(resolve=>{try{const socket=new WebSocket(denied.replace('http:','ws:'));const timer=setTimeout(()=>{socket.close();resolve(false);},1500);socket.onopen=()=>{clearTimeout(timer);socket.close();resolve(false);};socket.onerror=socket.onclose=()=>{clearTimeout(timer);resolve(true);};}catch{resolve(true);}});
      const registration=await navigator.serviceWorker.register('/sw.js').catch(()=>null);
      const service_worker_blocked=!registration && (await navigator.serviceWorker.getRegistrations()).length===0 && navigator.serviceWorker.controller===null;
      return {fetch_blocked,websocket_blocked,service_worker_blocked};
    },denied);
    return {...result,forbidden_requests:forbiddenRequests-initial,service_worker_requests:swRequests-initialSw};
  }
  if(name==='checkout_preview'){exact(args,[]);return {fidelity:'Model',currency:'fixture-units',quantity:1,predicted_total:1,dispatch:false};}
  if(name==='close'){exact(args,[]);return {closed:true};}
  throw Error('unsupported method');
}
async function writeFrame(value){const bytes=Buffer.from(JSON.stringify(value));if(bytes.length>FRAME)throw Error('frame limit');const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);await new Promise((resolve,reject)=>process.stdout.write(Buffer.concat([header,bytes]),error=>error?reject(error):resolve()));}
let buffer=Buffer.alloc(0),next=1,closing=false;
try {
  for await(const chunk of process.stdin){
    if(buffer.length+chunk.length>FRAME+4)throw Error('buffer limit');buffer=Buffer.concat([buffer,chunk]);
    while(buffer.length>=4){
      const length=buffer.readUInt32BE();if(!length||length>FRAME)throw Error('frame limit');if(buffer.length<length+4)break;
      const request=JSON.parse(buffer.subarray(4,length+4));buffer=buffer.subarray(length+4);
      exact(request,['version','id','method','args']);
      if(request.version!==1||request.id!==next++||!Number.isSafeInteger(request.id)||request.id>4294967295)throw Error('request id');
      try{const result=await method(request.method,request.args);await writeFrame({version:1,id:request.id,ok:true,result});}
      catch{await writeFrame({version:1,id:request.id,ok:false,error:'unsupported-or-invalid-request'});}
      if(request.method==='close'){closing=true;break;}
    }
    if(closing)break;
  }
} finally {
  await browser.close();fixture.closeAllConnections();forbidden.closeAllConnections();
  await Promise.all([new Promise(resolve=>fixture.close(resolve)),new Promise(resolve=>forbidden.close(resolve))]);
}
