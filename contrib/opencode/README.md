# hudbar-notify: уведомления opencode → dunst

Глобальный плагин opencode. Слушает события сессии и шлёт desktop-уведомления
через `notify-send` в демон `dunst`. Позже транспорт можно заменить на нативный
попап HUDbar — для этого есть единственная функция `deliver()` в `lib/deliver.js`.

## События

- `session.status === "busy"` — помечаем сессию работающей, без уведомления.
- `session.idle` — «Готово», только если сессию видели busy. Субагенты молчат.
- `permission.updated` — «Нужно разрешение», urgency critical. Субагенты не
  молчат, добавляется пометка «(субагент)». Дедуп по `properties.id`.
- `permission.replied` — снимает дедуп по `permissionID`.
- `question.asked` (и любой `question*.asked`) — «Агент задал вопрос»,
  critical, текст из первого вопроса. Если в 1.18.35 событие не приходит,
  отладочный лог это покажет.
- `session.error` — «Ошибка», critical. `sessionID` и `error` там опциональны:
  без id показываем общий текст. Субагенты молчат.

Не уведомляем, если окно с этим opencode в фокусе: берём pid окна из
`niri msg --json focused-window` и проверяем, входит ли он в цепочку родителей
нашего процесса (`/proc/<pid>/stat`, разбор по ПОСЛЕДНЕЙ `)`).

Троттлинг: не чаще 1 уведомления на пару (sessionID, kind) за 5 с; глобально
не больше 3 «готово» в минуту. Critical-типы лимит обходят.

## Структура

```
contrib/opencode/
  hudbar-notify.js   точка входа, ровно ОДИН экспорт HudbarNotify
  lib/
    decide.js        таблица решений (чистая, тестируется)
    throttle.js      троттлинг и память разрешений (чистая, тестируется)
    strings.js       тексты, экранирование разметки dunst, усечение
    proc.js          разбор /proc/<pid>/stat, цепочка предков
    focus.js         проверка фокуса через niri + /proc
    deliver.js       notify-send через argv, таймаут, обработка отказов
    debug.js         отладочный лог
  test/
    *.test.js        node --test
    live-niri.test.js  живой niri, в обычном прогоне НЕ запускается
```

## Установка (fish)

Исходник живёт в репозитории, в `~/.config/opencode/plugins/` ставится
симлинк. Старый `notify-idle.js` при этом снимаем, чтобы не было дубля
уведомлений — переименовываем в `.bak`, не удаляем.

```fish
mkdir -p ~/.config/opencode/plugins
mv ~/.config/opencode/plugins/notify-idle.js ~/.config/opencode/plugins/notify-idle.js.bak
ln -s /home/mihail/code/hudbar/contrib/opencode/hudbar-notify.js ~/.config/opencode/plugins/hudbar-notify.js
```

Затем перезапустить opencode. В логе должно появиться `hudbar-notify` /
«плагин загружен». Проверка вручную:

```fish
node -e "import('/home/mihail/code/hudbar/contrib/opencode/lib/deliver.js').then(async ({deliver}) => console.log(JSON.stringify(await deliver('Готово','проверка',{urgency:'low',tag:'opencode-manual-test'}))))"
```

## Откат (fish)

```fish
rm ~/.config/opencode/plugins/hudbar-notify.js
mv ~/.config/opencode/plugins/notify-idle.js.bak ~/.config/opencode/plugins/notify-idle.js
```

Перезапустить opencode. Репозиторий при этом не трогается.

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

Пишет в `~/.cache/hudbar/opencode-notify.log` строку на КАЖДОЕ входящее
событие: время, тип, sessionID, решение и причину. Содержимое сообщений
не пишется никогда — только типы и id. Без переменной файл не создаётся.

Проверить, приходят ли `question.*` в этой версии: включить режим,
спровоцировать вопрос агента, посмотреть лог. Если строк
`event=question.asked` нет — событие в 1.18.35 не эмитится.

## Тесты

```fish
cd /home/mihail/code/hudbar/contrib/opencode
node --test test/proc.test.js test/strings.test.js test/throttle.test.js test/decide.test.js test/deliver.test.js test/focus-debug.test.js
```

75 тестов. Живой вызов niri висит в песочнице раннера (из shell отвечает
мгновенно), поэтому вынесен в `test/live-niri.test.js` и в прогон не входит.
Запуск вручную: `node --test test/live-niri.test.js`.

Тесты НЕ трогают живой dunst: вместо `notify-send` подставляется заглушка
через `options.program`. Одна ручная отправка делается отдельно при приёмке.

## Безопасность

- Только argv, никакого shell. Заголовок сессии может содержать
  `$(touch /tmp/x)` — через массив аргументов это просто строка.
- `--` перед заголовком и телом: заголовок на `-` не станет опцией.
- Заголовок сессии экранируется (`&`, `<`, `>`), т.к. dunst разбирает разметку.
- Любая ошибка гасится и пишется через `client.app.log` (service
  `hudbar-notify`), opencode не падает никогда.
