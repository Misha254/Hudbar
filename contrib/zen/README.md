# Живая тема Zen Browser

Zen не перечитывает `userChrome.css` и сам перекрывает Firefox-тему своей
темой рабочей области. Поэтому цвета обновляет расширение **Omafox**: matugen
пишет JSON, native-host отдаёт его расширению, а оно меняет стандартную
Firefox-тему на лету. Статический мост `omafox.css` переводит её переменные
`--lwt-*` в переменные Zen.

```
wall.sh -> matugen -> <профиль Zen>/matugen-theme.json
                              | inotify
                    omafox-native-host
                              | native messaging
                    расширение Omafox -> browser.theme.update()
                              |
                    chrome/omafox.css
```

## Установка

```bash
./install.sh --zen
```

или отдельно:

```bash
contrib/zen/install.sh
```

Скрипт сам определяет flatpak (`~/.var/app/app.zen_browser.zen/.zen`) или
обычную установку (`~/.zen`) и ставит в профиль по умолчанию:

- `chrome/omafox.css` — мост `--lwt-*` → `--zen-*` (плюс переопределение
  inline-градиента `#zen-browser-background`, который Zen 1.24 пишет сам);
- импорт моста в `chrome/userChrome.css` (файл пользователя только
  дополняется, не заменяется);
- `extensions/@omafox.xpi` — подписанное расширение с AMO;
- префы `zen.theme.disable-lightweight=false` и
  `extensions.autoDisableScopes=0` в `user.js`;
- native-host и манифест в `~/.mozilla/native-messaging-hosts/` (для flatpak —
  внутри `~/.var/app/app.zen_browser.zen/.mozilla/`);
- шаблон matugen и запись `[templates.zen]` в `config.toml`.

Повторный запуск идемпотентен. Изменённые файлы один раз сохраняются рядом с
суффиксом `.hudbar-backup`.

После установки **перезапусти Zen один раз** — расширение и `userChrome.css`
читаются при старте. Дальше цвета меняются без перезапуска, задержка около
секунды.

## Требования

- Linux, Zen Browser 1.24+ (flatpak или обычная установка);
- `cc` (gcc) — native-host собирается из исходника;
- `matugen` с настроенным `~/.config/matugen/config.toml`;
- `python3` — только для правки `config.toml`; без него скрипт печатает
  готовый блок для ручной вставки.

## Откат

1. Удалить расширение Omafox в `about:addons` — мост работает только при
   `:root[lwtheme]`, и Zen вернётся к своей теме.
2. Вернуть файлы из `.hudbar-backup`.
3. Для flatpak: `flatpak override --user --reset app.zen_browser.zen`.

## Границы

- Режим в шаблоне зафиксирован: `"mode": "dark"`. Для светлой схемы matugen
  нужно поменять на `"light"` и перегенерировать JSON.
- Градиент рабочей области заменяется плоским цветом палитры — иначе Zen
  перекрывает matugen своим inline-градиентом.
- Сайты и поле поиска на новой вкладке остаются своих цветов: красится chrome
  и фон новой вкладки.
- Расширение подписано и не использует сеть; права — `nativeMessaging`,
  `storage`, `theme`.

## Источники

`omafox.css`, `omafox-native-host.c` и `omafox.xpi` — из проекта
[Omafox](https://github.com/cover/omafox) (MIT, см. `LICENSE.omafox`).
В host изменён путь состояния на `$HOME/.zen/matugen-theme.json`, в CSS
добавлено правило для `#zen-browser-background`.
