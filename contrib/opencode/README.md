# hudbar-notify: уведомления opencode → dunst

Глобальный плагин opencode. Слушает события сессии и шлёт desktop-уведомления
через `notify-send` в демон `dunst`. Позже транспорт можно заменить на нативный
попап HUDbar — для этого есть единственная функция `deliver()` в `lib/deliver.js`.

## API: opencode v2

Плагин написан под v2 (проверено на 2.0.28 и 2.2.0). Модуль экспортирует
по умолчанию определение с `id` и `setup(ctx)`; хелперы наружу не
экспортируются, иначе загрузчик примет их за отдельные плагины.

```js
export default {
  id: "hudbar-notify",
  setup(ctx) {
    const controller = new AbortController()
    void (async () => {
      for await (const event of ctx.event.subscribe({ signal: controller.signal })) { /* ... */ }
    })()
    return () => controller.abort()
  },
}
```

Что поменялось против v1: `client` → доменные методы контекста
(`ctx.session.get`), `event`-хук → `ctx.event.subscribe()`, полезная нагрузка
события лежит в `event.data` (не в `properties`), `client.app.log` в v2 нет —
журнал ведёт `lib/debug.js`, а `console.log` из серверного плагина писал бы
прямо в терминал и портил TUI.

Горячая перезагрузка плагина по watcher'у **не помогает**: Bun кэширует уже
импортированный модуль по пути, поэтому после правки файла opencode продолжает
видеть старую версию (и старую ошибку загрузки). Нужен перезапуск:

```fish
opencode service restart
```

## События

Сервер отдаёт `{ type, data }`. Обрабатываются:

- `session.execution.started` — помечаем сессию работающей, без уведомления.
  (`session.status` со `status.type === "busy"` — то же самое на старых сборках.)
- `session.execution.succeeded` / `session.execution.interrupted` / `session.idle` —
  «Готово», только если сессию видели работающей. Субагенты молчат. Три события
  на одну сессию дублируют друг друга, но помечка снимается первым же, поэтому
  уведомление всё равно одно.
- `permission.asked` — «Нужно разрешение», urgency critical. Субагенты не
  молчат, добавляется пометка «(субагент)». Дедуп по `data.id`, текст — из
  `data.message`.
- `permission.replied` — снимает дедуп по `data.requestID`.
- `form.created` с `metadata.kind === "question"` — «Агент задал вопрос»,
  critical. Текст берётся из `data.form.fields[0].description` (сам вопрос),
  при пустом — из `title` поля, и только потом заголовок сессии. Заголовок
  формы не используется: у вопросов он служебный «Questions».
- `form.replied` / `form.cancelled` — снимают дедуп по `data.id`, чтобы
  повторное событие по той же форме не пикало снова.
- `session.error` и `session.execution.failed` — «Ошибка», critical. У
  `session.error` sessionID опционален: без него показываем общий текст.
  Субагенты молчат.

### Прочие формы — молчат

`form.created` приходит не только на вопросы. В 2.0.28 так создаются:

| `metadata.kind` | `form.title` | Что это |
| --- | --- | --- |
| `question` | `Questions` | вопрос агента, `metadata.tool` = вызов tool'а |
| `websearch.provider` | `Web Search`, `Choose a web search provider` | выбор провайдера веб-поиска |
| `mcp-elicitation` | `<server> is requesting input` | MCP-сервер просит ввод |

Без проверки `kind` плагин писал «Агент задал вопрос / Web Search» на каждую
первую веб-поисковую сессию, хотя вопроса никто не задавал. Сейчас уведомляем
только `kind === "question"`; остальные виды форм просто пишутся в отладочный
журнал. Чтобы начать уведомлять и про них — расширь `isQuestionEvent()` в
`lib/decide.js` и добавь свой `kind` в `STRINGS.titles`.

Субагент определяется по `parentID` из `ctx.session.get({ sessionID })`; ответ
кэшируется вместе с заголовком и сбрасывается на `session.renamed`.

Не уведомляем, если окно с этим opencode в фокусе: берём pid окна из
`niri msg --json focused-window` и проверяем, входит ли он в цепочку родителей
нашего процесса (`/proc/<pid>/stat`, разбор по ПОСЛЕДНЕЙ `)`).

Троттлинг: не чаще 1 уведомления на пару (sessionID, kind) за 5 с; глобально
не больше 3 «готово» в минуту. Critical-типы лимит обходят.

### Время жизни на экране

`notify-send` вызывается с явным `-t`: 8 с для «готово», 20 с для вопросов,
разрешений и ошибок. Без `-t` уведомление живёт бесконечно: в `~/.config/dunst/dunstrc`
не задан `timeout`, а critical dunst сам не гасит — проверено на машине,
`notify-send -u critical` висел дольше минуты, `-t 5000` закрылся через пять
секунд. Значения лежат в `TIMEOUT_MS` (`lib/decide.js`), дефолт транспорта —
`DEFAULT_TIMEOUT_MS` (`lib/deliver.js`).

## Структура

```
contrib/opencode/
  hudbar-notify.js   точка входа, ОДИН экспорт по умолчанию { id, setup }
  lib/
    decide.js        таблица решений (чистая, тестируется)
    throttle.js      троттлинг и память разрешений (чистая, тестируется)
    strings.js       тексты, экранирование разметки dunst, усечение
    proc.js          разбор /proc/<pid>/stat, цепочка предков
    focus.js         проверка фокуса через niri + /proc
    deliver.js       notify-send через argv, таймаут, обработка отказов
    debug.js         отладочный журнал: события и тексты ошибок
  test/
    *.test.js        node --test
    live-niri.test.js  живой niri, в обычном прогоне НЕ запускается
```

## Установка (fish)

Исходник живёт в репозитории, в `~/.config/opencode/plugins/` ставится
симлинк (команды — из корня репозитория). Старый `notify-idle.js` при этом
снимаем, чтобы не было дубля уведомлений — переименовываем в `.bak`, не
удаляем.

```fish
mkdir -p ~/.config/opencode/plugins
mv ~/.config/opencode/plugins/notify-idle.js ~/.config/opencode/plugins/notify-idle.js.bak
ln -s "$PWD/contrib/opencode/hudbar-notify.js" ~/.config/opencode/plugins/hudbar-notify.js
```

В 2.2.0 сервер сам следит за каталогом и загружает новый файл без
перезапуска; `opencode service restart` нужен, только если этого не
произошло.

Проверка, что плагин жив:

```fish
opencode api get /api/plugin | grep -o '"id":"hudbar-notify"[^}]*'
```

В ответе должно быть `"state":{"status":"active"}`. `opencode plugin list`
для этого не годится: он показывает только плагины-пакеты из `plugins`
конфига, а локальный файл в `~/.config/opencode/plugins/` в нём не
появляется — даже когда загружен.

Ручная отправка уведомления мимо opencode:

```fish
node -e "import('$PWD/contrib/opencode/lib/deliver.js').then(async ({deliver}) => console.log(JSON.stringify(await deliver('Готово','проверка',{urgency:'low',tag:'opencode-manual-test'}))))"
```

## Откат (fish)

```fish
rm ~/.config/opencode/plugins/hudbar-notify.js
mv ~/.config/opencode/plugins/notify-idle.js.bak ~/.config/opencode/plugins/notify-idle.js
```

Сервер снимет плагин сам; `opencode service restart` — если каталог не
отследился.

Репозиторий при этом не трогается.

## Секция dunstrc (необязательно)

Конфиг dunst НЕ правим сами — сниппет ниже на случай, если захочется
отдельную секцию. Добавляется в `~/.config/dunst/dunstrc` вручную:

```
[opencode]
    appname = "opencode"
    summary = "*"
    history_ignore = false
    set_stack_tag = opencode
```

Плагин и без неё работает: тег уже передаётся через
`string:x-dunst-stack-tag`, а `history_ignore = false` и так по умолчанию.
Секция нужна, только если захочется особые иконки или таймауты.

## Отладочный режим

```fish
set -x HUDBAR_NOTIFY_DEBUG 1
```

Пишет в `~/.cache/hudbar/opencode-notify.log`:

- строку на КАЖДОЕ входящее событие: время, тип, sessionID, решение, причина;
- строки `log level=...` на ошибки плагина и отказы notify-send.

Содержимое сообщений не пишется никогда — только типы и id. Без переменной
файл не создаётся.

## Тесты

```fish
cd contrib/opencode
node --test test/proc.test.js test/strings.test.js test/throttle.test.js test/decide.test.js test/deliver.test.js test/focus-debug.test.js
```

107 тестов. Живой вызов niri висит в песочнице раннера (из shell отвечает
мгновенно), поэтому вынесен в `test/live-niri.test.js` и в прогон не входит.
Запуск вручную: `node --test test/live-niri.test.js`.

Тесты НЕ трогают живой dunst: вместо `notify-send` подставляется заглушка
через `options.program`. Одна ручная отправка делается отдельно при приёмке.

## Безопасность

- Только argv, никакого shell. Заголовок сессии может содержать
  `$(touch /tmp/x)` — через массив аргументов это просто строка.
- `--` перед заголовком и телом: заголовок на `-` не станет опцией.
- Заголовок сессии экранируется (`&`, `<`, `>`), т.к. dunst разбирает разметку.
- Любая ошибка гасится и уходит в отладочный журнал, opencode не падает
  никогда.