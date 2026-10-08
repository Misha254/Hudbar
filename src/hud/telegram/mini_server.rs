//! Маршрутизация Mini App: путь и тело запроса → ответ.
//!
//! Мост отдаёт ровно шесть эндпоинтов, и ни один из них не даёт доступа к
//! файлам или командам shell. Набор маленький не случайно: на публичном
//! адресе каждый лишний эндпоинт — лишняя поверхность атаки.

use serde_json::{Value, json};

use super::mini::{self, ClientShared, Reply, Request, StateShared};
use super::opencode::{Client, ModelRef};

/// Что нужно мосту для ответа: клиент opencode, состояние и токен.
pub struct Deps<'a> {
    /// Клиент opencode под мьютексом: сетевые вызовы держат его недолго.
    pub client: &'a ClientShared,
    /// Состояние моста: сессия, папка, модель.
    pub state: &'a StateShared,
    /// Токен доступа: без него `/api` молчит.
    pub token: Option<&'a str>,
}

/// Ответ маршрута.
pub type Routed = Reply;

/// Разбор пути и вызов обработчика.
pub fn route(deps: &Deps<'_>, request: &Request) -> Routed {
    // Страница отдаётся без токена: без него она бесполезна, все данные за
    // `/api`. Так ссылку можно открыть и увидеть, что панель жива.
    if request.path == "/" || request.path == "/index.html" {
        return Reply::html(&super::mini_page::page());
    }
    if request.path == "/health" {
        return Reply::json(200, &json!({ "ok": true }));
    }
    let Some(secret) = deps.token else {
        return mini::fail_json(503, "токен не задан: панель выключена");
    };
    // Токен принимается заголовком, а из query — только для потока событий:
    // `EventSource` не умеет заголовки, а фрагмент живёт лишь в основном
    // запросе. В обычных запросах токен в query уехал бы в лог туннеля.
    let from_header = request.header(mini::TOKEN_HEADER).map(str::to_string);
    let from_query = secret_in_query(&request.query);
    let is_events = request.path == "/api/events";
    let ok = |value: Option<&String>| value.is_some_and(|value| mini::tokens_match(value, secret));
    let allowed = ok(from_header.as_ref()) || (is_events && ok(from_query.as_ref()));
    if !allowed {
        return mini::fail_json(401, "нет токена или он не подошёл");
    }
    let Ok(query) = deps.client.lock() else {
        return mini::fail_json(503, "мост занят");
    };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/state") => state_answer(deps, &query),
        ("GET", "/api/sessions") => sessions_answer(&query),
        ("GET", "/api/models") => models_answer(&query),
        // Обработчики с побочным эффектом возвращают `Result`, где ошибка уже
        // готова к отправке: наружу уходит она сама, а не код.
        ("POST", "/api/prompt") => unwrap(prompt_answer(deps, &query, request)),
        ("POST", "/api/stop") => unwrap(stop_answer(deps, &query)),
        ("POST", "/api/session") => unwrap(session_answer(deps, &query, request)),
        ("POST", "/api/session/new") => unwrap(new_session_answer(deps, &query)),
        ("POST", "/api/model") => unwrap(model_answer(deps, &query, request)),
        // Поток: часы обновляются на месте, без опроса сервера.
        ("GET", "/api/events") => Reply::events(),
        _ => mini::fail_json(404, "нет такого эндпоинта"),
    }
}

/// Ответ обработчика: успех и ошибка уже оформлены, остаётся снять `Result`.
fn unwrap(answer: Result<Routed, Routed>) -> Routed {
    answer.unwrap_or_else(|reply| reply)
}

/// Токен в query-строке: `t=…`. Фрагмент не уходит серверу, а для потока
/// событий заголовок недоступен, поэтому это единственный путь.
fn secret_in_query(query: &str) -> Option<String> {
    let raw = query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == mini::TOKEN_FRAGMENT).then_some(value)
    })?;
    // Пустой токен — не токен: иначе `?t=` открыл бы панель без секрета.
    percent_decode(raw).filter(|value| !value.is_empty())
}

/// Разбор `%`-кодирования: токен может состоять из байт, которые пришлось
/// закодировать, иначе `=` или `&` внутри сломали бы разбор.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = raw.get(index + 1..index + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// `/api/state`: активная сессия, модель, работает ли агент, папка. Тот же
/// ответ уходит и в поток событий, поэтому он собирается в строку.
pub(crate) fn state_payload(deps: &Deps<'_>, client: &Client) -> String {
    state_answer(deps, client)
        .body
        .iter()
        .map(|byte| *byte as char)
        .collect()
}

fn state_answer(deps: &Deps<'_>, client: &Client) -> Routed {
    let session = read_session(deps.state);
    let wanted_id = session.clone();
    let busy = session
        .as_deref()
        .and_then(|wanted| {
            let statuses = client.session_status().ok()?;
            statuses.get(wanted).cloned()
        })
        .is_some_and(|kind| kind == "busy" || kind == "retry");
    let model = session
        .as_deref()
        .and_then(|wanted| client.session_model(wanted).ok())
        .flatten();
    let title = match client.list_sessions(20) {
        Ok(sessions) => match wanted_id.as_deref() {
            Some(wanted) => sessions
                .iter()
                .find(|item| item.id == wanted)
                .map_or_else(mini::no_session, mini::session_json),
            None => mini::no_session(),
        },
        // Сервер недоступен: шапка панели покажет «нет сессии», но сама панель
        // останется живой — об этом скажет подсказка об ошибке.
        Err(_) => mini::no_session(),
    };
    let directory = deps
        .state
        .lock()
        .map(|state| state.directory.clone())
        .unwrap_or_default();
    mini::ok_json(json!({
        "session": title,
        "sessionId": wanted_id,
        "model": model.as_ref().map(|model| model.id.clone()),
        "modelProvider": model.as_ref().map(|model| model.provider.clone()),
        "modelFull": model.as_ref().map(ModelRef::full_id),
        "directory": directory,
        "busy": busy,
    }))
}

/// `/api/sessions`: последние сессии папки.
fn sessions_answer(client: &Client) -> Routed {
    match client.list_sessions(20) {
        Ok(sessions) => mini::ok_json(json!({
            "sessions": sessions.iter().map(mini::session_json).collect::<Vec<Value>>(),
        })),
        Err(why) => mini::fail_json(502, &short(&why)),
    }
}

/// `/api/models`: подключённые провайдеры, по провайдеру и внутри по id.
fn models_answer(client: &Client) -> Routed {
    match client.list_models() {
        Ok(models) => mini::ok_json(json!({
            "models": models.iter().map(mini::model_json).collect::<Vec<Value>>(),
        })),
        Err(why) => mini::fail_json(502, &short(&why)),
    }
}

/// `/api/prompt`: промпт в активную сессию. Сессии нет — создаётся, как и в
/// боте: панель это тот же мост, а не отдельный вход.
fn prompt_answer(deps: &Deps<'_>, client: &Client, request: &Request) -> Result<Routed, Routed> {
    let text = mini::prompt_from(request).map_err(|why| mini::fail_json(400, &why))?;
    let session = match read_session(deps.state) {
        Some(session) => session,
        None => match client.create_session_modeled(None, read_model(deps.state).as_ref()) {
            Ok(created) => created.id,
            Err(why) => return Err(mini::fail_json(502, &short(&why))),
        },
    };
    if let Some(model) = read_model(deps.state)
        && let Err(why) = client.set_session_model(&session, &model)
    {
        // Модель выбрана, но сервер её не принял: промпт всё равно уйдёт на
        // текущую модель сессии, поэтому это предупреждение, а не отказ.
        super::log::warn(format!("панель: модель не применилась: {why}"));
    }
    client
        .send_prompt_async(&session, &text)
        .map_err(|why| mini::fail_json(502, &short(&why)))?;
    write_session(deps.state, Some(session.clone()));
    Ok(mini::ok_json(json!({ "sessionId": session })))
}

/// `/api/stop`: прервать работу агента в активной сессии.
fn stop_answer(deps: &Deps<'_>, client: &Client) -> Result<Routed, Routed> {
    let Some(session) = read_session(deps.state) else {
        return Ok(mini::ok_json(
            json!({ "stopped": false, "why": "сессии нет" }),
        ));
    };
    // Карточку в Telegram обновит поток событий по `session.idle`: отдельного
    // вызова из панели не делаем, чтобы состояние менялось в одном месте.
    client
        .abort_session(&session)
        .map_err(|why| mini::fail_json(502, &short(&why)))?;
    Ok(mini::ok_json(json!({ "stopped": true })))
}

/// `/api/session`: перейти по id сессии. Модель выбранная применяется сразу,
/// иначе панель переключала бы сессию и забывала модель.
fn session_answer(deps: &Deps<'_>, client: &Client, request: &Request) -> Result<Routed, Routed> {
    let id = request
        .field("id")
        .ok_or_else(|| mini::fail_json(400, "не указана сессия"))?;
    let session = client
        .get_session(&id)
        .map_err(|why| mini::fail_json(502, &short(&why)))?
        .ok_or_else(|| mini::fail_json(404, "такой сессии нет"))?;
    write_session(deps.state, Some(session.id.clone()));
    if let Some(model) = read_model(deps.state)
        && let Err(why) = client.set_session_model(&session.id, &model)
    {
        super::log::warn(format!("панель: модель не перенеслась в сессию: {why}"));
    }
    Ok(mini::ok_json(json!({ "sessionId": session.id })))
}

/// `/api/session/new`: новая сессия в папке моста, на выбранной модели.
fn new_session_answer(deps: &Deps<'_>, client: &Client) -> Result<Routed, Routed> {
    let created = client
        .create_session_modeled(None, read_model(deps.state).as_ref())
        .map_err(|why| mini::fail_json(502, &short(&why)))?;
    write_session(deps.state, Some(created.id.clone()));
    Ok(mini::ok_json(json!({ "sessionId": created.id })))
}

/// `/api/model`: выбрать модель по id. Применяется к активной сессии и
/// запоминается, чтобы следующие промпты и новые сессии шли на ней.
fn model_answer(deps: &Deps<'_>, client: &Client, request: &Request) -> Result<Routed, Routed> {
    let id = request
        .field("id")
        .ok_or_else(|| mini::fail_json(400, "не указана модель"))?;
    let models = client
        .list_models()
        .map_err(|why| mini::fail_json(502, &short(&why)))?;
    let Some(model) = models
        .iter()
        .find(|model| model.reference.id == id)
        .cloned()
    else {
        return Err(mini::fail_json(404, "такой модели нет"));
    };
    if let Some(session) = read_session(deps.state)
        && let Err(why) = client.set_session_model(&session, &model.reference)
    {
        return Err(mini::fail_json(502, &short(&why)));
    }
    write_model(deps.state, Some(model.reference.clone()));
    Ok(mini::ok_json(json!({ "model": model.reference.full_id() })))
}

/// Активная сессия из состояния.
fn read_session(state: &StateShared) -> Option<String> {
    state.lock().ok()?.session.clone()
}

/// Выбранная модель из состояния.
fn read_model(state: &StateShared) -> Option<ModelRef> {
    state.lock().ok()?.model.clone()
}

/// Активную сессию в состояние.
fn write_session(state: &StateShared, session: Option<String>) {
    if let Ok(mut state) = state.lock() {
        state.session = session;
    }
}

/// Выбранную модель в состояние.
fn write_model(state: &StateShared, model: Option<ModelRef>) {
    if let Ok(mut state) = state.lock() {
        state.model = model;
    }
}

/// Короткая причина: текст ошибки opencode длинный, а на телефоне важно, что
/// именно сломалось, а не где в стеке.
fn short(why: &str) -> String {
    let line = why.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        "неизвестная ошибка".to_string()
    } else {
        line.chars().take(160).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_token_is_parsed_and_percent_decoded() {
        assert_eq!(secret_in_query("t=abc123"), Some("abc123".to_string()));
        assert_eq!(
            secret_in_query("other=1&t=a%2Bb"),
            Some("a+b".to_string()),
            "%-кодирование и знак плюса"
        );
        assert_eq!(secret_in_query("t="), None, "пустой токен — не токен");
        assert_eq!(secret_in_query("x=1"), None);
    }

    #[test]
    fn percent_decode_survives_garbage() {
        assert_eq!(percent_decode("%zz"), None, "не hex");
        assert_eq!(percent_decode("%4"), None, "обрезанная пара");
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("обычный"), Some("обычный".to_string()));
    }

    /// Страница отдаётся без токена: это не данные, а пустая оболочка, а все
    /// данные живут за `/api` и без токена не отдаются.
    #[test]
    fn page_needs_no_token_but_api_does() {
        let (state, client) = new_shared();
        let deps = Deps {
            client: &client,
            state: &state,
            token: Some("секрет"),
        };
        let page = route(&deps, &Request::test_get("/", None));
        assert_eq!(page.status, 200, "страница открыта без токена");
        assert!(String::from_utf8_lossy(&page.body).contains("doctype"));

        let api = route(&deps, &Request::test_get("/api/state", None));
        assert_eq!(api.status, 401, "данные без токена не отдаются");
        assert!(String::from_utf8_lossy(&api.body).contains("нет токена"));
    }

    #[test]
    fn wrong_token_is_refused_for_every_api_path() {
        let (state, client) = new_shared();
        let deps = Deps {
            client: &client,
            state: &state,
            token: Some("секрет"),
        };
        for path in [
            "/api/state",
            "/api/sessions",
            "/api/models",
            "/api/prompt",
            "/api/stop",
            "/api/session",
            "/api/model",
            "/api/events",
        ] {
            let reply = route(
                &deps,
                &Request::test_post(path, Some("не тот"), r#"{"prompt":"x"}"#),
            );
            assert_eq!(reply.status, 401, "{path} с чужим токеном");
        }
    }

    #[test]
    fn without_a_configured_token_the_panel_is_off() {
        let (state, client) = new_shared();
        let deps = Deps {
            client: &client,
            state: &state,
            token: None,
        };
        let reply = route(&deps, &Request::test_get("/api/state", Some("любой")));
        assert_eq!(reply.status, 503, "токен не задан — панель выключена");
        assert!(String::from_utf8_lossy(&reply.body).contains("выключена"));
        // Страница при этом жива: человек видит, что ссылка открылась.
        assert_eq!(route(&deps, &Request::test_get("/", None)).status, 200);
    }

    /// Токен в query принимается только для потока событий: там заголовок
    /// недоступен, а в обычных запросах это лишний способ утечь токен в лог.
    #[test]
    fn query_token_works_for_the_event_stream_only() {
        let (state, client) = new_shared();
        let deps = Deps {
            client: &client,
            state: &state,
            token: Some("секрет"),
        };
        let mut request = Request::test_get("/api/events", None);
        request.query = "t=секрет".to_string();
        assert_eq!(
            route(&deps, &request).status,
            200,
            "поток пускает с токеном в query"
        );

        let mut request = Request::test_post("/api/prompt", None, r#"{"prompt":"x"}"#);
        request.query = "t=секрет".to_string();
        let reply = route(&deps, &request);
        assert_ne!(
            reply.status, 200,
            "обычный запрос с токеном в query не проходит"
        );
    }

    #[test]
    fn unknown_paths_and_methods_are_rejected() {
        let (state, client) = new_shared();
        let deps = Deps {
            client: &client,
            state: &state,
            token: Some("секрет"),
        };
        let reply = route(&deps, &Request::test_get("/api/secret", Some("секрет")));
        assert_eq!(reply.status, 404);
        assert!(String::from_utf8_lossy(&reply.body).contains("нет такого эндпоинта"));
        // Чтение файлов и запуск команд в списке просто отсутствуют.
        for path in ["/api/file", "/api/exec", "/api/shell", "/../etc/passwd"] {
            assert_eq!(
                route(&deps, &Request::test_get(path, Some("секрет"))).status,
                404,
                "{path} не существует"
            );
        }
        let mut request = Request::test_get("/api/state", Some("секрет"));
        request.method = "DELETE".to_string();
        assert_eq!(
            route(&deps, &request).status,
            404,
            "метод не тот — эндпоинта нет"
        );
    }

    #[test]
    fn health_needs_no_token_but_leaks_nothing() {
        let (state, client) = new_shared();
        let deps = Deps {
            client: &client,
            state: &state,
            token: Some("секрет"),
        };
        let reply = route(&deps, &Request::test_get("/health", None));
        assert_eq!(reply.status, 200);
        let value: Value = serde_json::from_slice(&reply.body).expect("json");
        assert_eq!(value["ok"], true);
        assert_eq!(value.as_object().expect("объект").len(), 1, "только флаг");
    }

    #[test]
    fn short_error_text_stays_one_line() {
        assert_eq!(short("первая строка\nвторая"), "первая строка");
        assert_eq!(short("   "), "неизвестная ошибка");
        let long: String = "x".repeat(500);
        assert_eq!(short(&long).chars().count(), 160, "обрезано");
    }

    /// Пустой мост без сервера: `/api/state` должен ответить, а не упасть.
    /// Сеанс живой — сервер на месте, и это проверяется кнопкой в боте.
    #[test]
    fn state_answer_works_against_a_dead_server() {
        let (state, dead) = new_shared();
        let guard = dead.lock().expect("клиент");
        let deps = Deps {
            client: &dead,
            state: &state,
            token: Some("t"),
        };
        let answer = state_answer(&deps, &guard);
        assert_eq!(answer.status, 200, "состояние отдаётся даже без сервера");
        let value: Value = serde_json::from_slice(&answer.body).expect("json");
        assert_eq!(value["ok"], true);
        assert_eq!(value["busy"], false);
        assert_eq!(value["sessionId"], Value::Null);
    }

    /// Мёртвый сервер должен давать ошибку, а не пустой список: иначе панель
    /// покажет «ничего не нашлось» и это будет выглядеть как поломка данных.
    #[test]
    fn lists_report_a_dead_server_instead_of_pretending_to_be_empty() {
        let (_state, client) = new_shared();
        let guard = client.lock().expect("клиент");
        for answer in [sessions_answer(&guard), models_answer(&guard)] {
            assert_eq!(answer.status, 502);
            let value: Value = serde_json::from_slice(&answer.body).expect("json");
            assert_eq!(value["ok"], false);
            assert!(
                value["error"].as_str().is_some_and(|text| !text.is_empty()),
                "причина есть"
            );
        }
    }

    /// Мьютексы моста для тестов: состояние и клиент на мёртвом порту,
    /// чтобы сетевые вызовы падали быстро и предсказуемо.
    fn new_shared() -> (StateShared, ClientShared) {
        let state = StateShared::new(std::sync::Mutex::new(
            super::super::commands::State::default(),
        ));
        let client = ClientShared::new(std::sync::Mutex::new(
            Client::new("http://127.0.0.1:1").expect("адрес"),
        ));
        (state, client)
    }
}
