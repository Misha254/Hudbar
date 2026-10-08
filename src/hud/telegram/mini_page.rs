//! Страница Mini App: одна строка HTML со встроенными стилем и скриптом.
//!
//! Почему так: страницу раздаёт сам мост, а не nginx, и файлов рядом нет.
//! Держать статику в виде константы удобно и для сборки, и для проверки: тест
//! видит тот же текст, что уедет в Telegram.
//!
//! Токен сюда не вшит. Он приходит во фрагменте адреса (`#t=…`), который
//! браузер серверу не отправляет, поэтому его нет ни в логах туннеля, ни в
//! кеше. Страница кладёт его в заголовок запросов и убирает из адреса через
//! `history.replaceState`, чтобы токен не остался в кнопке «назад» и не уехал
//! в историю браузера.

/// Тема берётся из настроек Telegram, но палитра остаётся своей: тёмная
/// с лавандово-голубым акцентом, как весь HUDbar.
pub const PALETTE: &str = "--bg: #121318; --panel: #1b1d24; --line: #2a2d37; --fg: #e6e6ef; --dim: #9a9dab; --accent: #b5c4ff; --bad: #ff8b8b; --ok: #8bd6a0;";

/// Страница целиком.
pub fn page() -> String {
    let mut html = String::with_capacity(16 * 1024);
    html.push_str("<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">");
    html.push_str("<title>opencode</title><style>");
    html.push_str(&format!(":root{{{PALETTE}}}"));
    html.push_str(
        "*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,-apple-system,'Segoe UI',sans-serif;
padding:env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left)}
header{position:sticky;top:0;z-index:2;background:rgba(18,19,24,.94);backdrop-filter:blur(8px);
border-bottom:1px solid var(--line);padding:10px 14px}
h1{margin:0;font-size:16px;font-weight:600}
.sub{color:var(--dim);font-size:12px;margin-top:2px;word-break:break-all}
main{padding:12px 14px 96px}
section{margin-bottom:18px}
h2{font-size:12px;text-transform:uppercase;letter-spacing:.08em;color:var(--dim);margin:0 0 6px}
.row{display:flex;gap:6px;overflow-x:auto;padding-bottom:4px;scrollbar-width:none}
.row::-webkit-scrollbar{display:none}
.chip{flex:0 0 auto;padding:7px 12px;border-radius:999px;border:1px solid var(--line);
background:var(--panel);color:var(--fg);font-size:14px;cursor:pointer;white-space:nowrap}
.chip[aria-pressed=true]{border-color:var(--accent);color:var(--accent)}
input[type=search]{width:100%;padding:9px 12px;border-radius:10px;border:1px solid var(--line);
background:var(--panel);color:var(--fg);font-size:15px;margin-bottom:8px}
.list{display:flex;flex-direction:column;gap:4px;max-height:44vh;overflow:auto}
.item{display:flex;gap:8px;align-items:center;padding:9px 11px;border-radius:10px;
background:var(--panel);border:1px solid transparent;font-size:14px;text-align:left;color:var(--fg);cursor:pointer}
.item[aria-selected=true]{border-color:var(--accent)}
.item .t{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.item .m{color:var(--dim);font-size:12px}
.dot{color:var(--accent)}
footer{position:fixed;left:0;right:0;bottom:0;padding:10px 14px calc(10px + env(safe-area-inset-bottom));
background:rgba(18,19,24,.96);border-top:1px solid var(--line)}
textarea{width:100%;min-height:66px;max-height:34vh;resize:vertical;padding:10px 12px;border-radius:12px;
border:1px solid var(--line);background:var(--panel);color:var(--fg);font:15px/1.4 inherit}
.bar{display:flex;gap:8px;margin-top:8px;align-items:center}
button.act{flex:0 0 auto;padding:9px 15px;border-radius:12px;border:1px solid var(--line);
background:var(--panel);color:var(--fg);font-size:14px;cursor:pointer}
button.go{flex:1;background:var(--accent);color:#10121a;border:0;font-weight:600}
button.go[disabled]{opacity:.45}
.hint{color:var(--dim);font-size:12px;margin-top:6px}
.err{color:var(--bad);font-size:13px;min-height:1em;padding:0 2px}",
    );
    html.push_str("</style></head><body>");
    html.push_str(
        "<header><h1>opencode</h1><div class=\"sub\" id=\"sub\">подключение…</div></header>
<main>
<section id=\"sec-session\"><h2>Сессия</h2><div class=\"row\" id=\"sessions\"></div></section>
<section id=\"sec-model\"><h2>Модель</h2>
<input type=\"search\" id=\"model-search\" placeholder=\"поиск модели\" autocomplete=\"off\">
<div class=\"list\" id=\"models\"></div></section>
</main>
<footer>
<textarea id=\"prompt\" placeholder=\"промпт агенту\"></textarea>
<div class=\"bar\">
<button class=\"act\" id=\"stop\">Стоп</button>
<button class=\"go\" id=\"send\">Отправить</button>
</div>
<div class=\"hint\" id=\"hint\"></div>
</footer>
<script>",
    );
    html.push_str(script());
    html.push_str("</script></body></html>");
    html
}

/// Скрипт страницы. Без сборщика и без фреймворков: страница одна, а лишние
/// зависимости в этом крейте тянули бы за собой половину npm.
fn script() -> &'static str {
    r##"const $=id=>document.getElementById(id);
const el={sub:$('sub'),sessions:$('sessions'),models:$('models'),search:$('model-search'),
prompt:$('prompt'),send:$('send'),stop:$('stop'),hint:$('hint')};
let state={session:null,model:null,models:[],token:''};

// Токен из фрагмента адреса: сервер его не видел, и в историю он попасть не должен.
function takeToken(){
  const raw=location.hash.replace(/^#/,'');
  const p=new URLSearchParams(raw.startsWith('t=')?raw:'t='+raw.replace(/^t=/,''));
  state.token=p.get('t')||'';
  if(state.token)history.replaceState(null,'',location.pathname+location.search);
  if(!state.token)el.hint.textContent='нет токена: открой ссылку с #t=…';
}
async function api(path,body){
  const opts={headers:{'X-Hudbar-Token':state.token}};
  if(body!==undefined){opts.method='POST';opts.body=JSON.stringify(body);}
  const r=await fetch(path,opts);
  const j=await r.json().catch(()=>({ok:false,error:'ответ не json'}));
  if(!j.ok)throw new Error(j.error||('ошибка '+r.status));
  return j;
}
function esc(s){return String(s??'').replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));}
function chip(label,on,active){const b=document.createElement('button');b.className='chip';
  b.textContent=label;b.setAttribute('aria-pressed',active?'true':'false');b.onclick=on;return b;}

async function loadState(){
  const s=await api('/api/state');
  state.session=s.session;state.model=s.model;
  el.sub.textContent=(s.session&&s.session.title? s.session.title : (s.session?s.session.id:'сессии нет'))
    +' · '+(s.model||'модель не выбрана')+(s.busy?' · работает':'');
  el.send.disabled=!s.busy?false:false;
}
async function loadSessions(){
  const d=await api('/api/sessions');
  el.sessions.replaceChildren();
  el.sessions.append(chip('➕ новая',async()=>{await act('/api/session/new');},false));
  d.sessions.forEach(s=>{
    const on=s.id===state.session?.id;
    el.sessions.append(chip((on?'● ':'')+(s.title||s.id.replace(/^ses_/,'').slice(0,10)),
      async()=>{await act('/api/session',{id:s.id});},on));
  });
}
function drawModels(){
  const q=el.search.value.trim().toLowerCase();
  const rows=state.models.filter(m=>(m.name+' '+m.id+' '+m.provider).toLowerCase().includes(q));
  el.models.replaceChildren();
  if(!rows.length){el.models.textContent='ничего не нашлось';return;}
  rows.forEach(m=>{
    const on=m.id===state.model;
    const b=document.createElement('button');b.className='item';
    b.setAttribute('aria-selected',on?'true':'false');
    b.innerHTML='<span class="t">'+(on?'<span class="dot">● </span>':'')+esc(m.name||m.id)+'</span><span class="m">'+esc(m.provider)+'</span>';
    b.onclick=async()=>{await act('/api/model',{id:m.id});};
    el.models.append(b);
  });
}
async function loadModels(){
  const d=await api('/api/models');
  state.models=d.models;drawModels();
}
async function act(path,body){
  try{await api(path,body);await refresh();el.hint.textContent='';}
  catch(e){el.hint.textContent=e.message;}
}
async function send(){
  const prompt=el.prompt.value.trim();
  if(!prompt)return;
  el.send.disabled=true;el.hint.textContent='отправляю…';
  try{await api('/api/prompt',{prompt});el.prompt.value='';el.hint.textContent='промпт ушёл, ответ придёт в бот';}
  catch(e){el.hint.textContent=e.message;}
  finally{el.send.disabled=false;await refresh();}
}
async function refresh(){try{await loadState();await loadSessions();await loadModels();}catch(e){el.hint.textContent=e.message;}}

el.send.onclick=send;
el.stop.onclick=async()=>{try{await api('/api/stop',{});el.hint.textContent='останавливаю';}catch(e){el.hint.textContent=e.message;}};
el.search.oninput=drawModels;
el.prompt.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key==='Enter')send();});
takeToken();refresh();
setInterval(()=>{if(state.token)loadState().catch(()=>{});},5000);
"##
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Страница — это страница: с корректным doctype, вьюпортом для телефона
    /// и разметкой, к которой привязан скрипт.
    #[test]
    fn page_is_a_mobile_ready_document() {
        let html = page();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("name=\"viewport\""));
        assert!(
            html.contains("viewport-fit=cover"),
            "без вырезов не обойтись"
        );
        assert!(html.ends_with("</html>"), "документ закрыт");
        for id in [
            "sub", "sessions", "models", "prompt", "send", "stop", "hint",
        ] {
            assert!(html.contains(&format!("id=\"{id}\"")), "нет элемента {id}");
        }
    }

    /// Токен не вшит в страницу. Это главное: вшитый токен уехал бы в кеш и
    /// в историю каждого, кто открыл панель.
    #[test]
    fn page_has_no_embedded_token() {
        let html = page();
        assert!(!html.contains("bot_token"), "токен бота не вшит");
        // Скрипт читает токен из фрагмента и убирает его из адреса.
        assert!(html.contains("location.hash"), "токен берётся из фрагмента");
        assert!(html.contains("replaceState"), "и убирается из адреса");
    }

    #[test]
    fn page_uses_hudbar_palette() {
        let html = page();
        assert!(html.contains("#b5c4ff"), "лавандово-голубой акцент");
        assert!(html.contains("#121318"), "тёмный фон");
        assert!(!html.contains("teal"), "палитра не подменена");
    }
}
