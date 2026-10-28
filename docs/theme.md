# Единая тема

Палитра одна на всю сессию: matugen считает её из обоев, а каждый компонент
читает свой файл. Здесь — что именно читает каждый, чем это проверено и где
остаются границы.

## Матрица

| Компонент | Файл | Как применяется |
| --- | --- | --- |
| niri | `~/.config/niri/colors.kdl` | подключается из `config.kdl`; niri перечитывает конфиг сам |
| HUDbar | `~/.config/hudbar/colors.css` | панель следит за mtime и подхватывает на лету |
| kitty | `~/.config/kitty/colors.conf` | `include` в `kitty.conf`; новые окна и `ctrl+shift+f5` |
| dunst | `~/.config/dunst/dunstrc` | `dunstctl reload` или перезапуск демона |
| rofi | `~/.config/rofi/colors.rasi` | при следующем запуске |
| btop | `~/.config/btop/themes/matugen.theme` | при следующем запуске |
| GTK3 | `~/.config/gtk-3.0/colors.css` | `gtk.css` импортирует `colors.css`; новые процессы |
| GTK4 / libadwaita | `~/.config/gtk-4.0/colors.css` | тот же импорт; переменные `:root`; новые процессы |
| Qt5 / Qt6 | — | `QT_QPA_PLATFORMTHEME=gtk3` в сессии, цвета берутся из GTK3 |
| cava | `~/.config/cava/themes/matugen` | при следующем запуске |
| nvim | `~/.config/nvim/after/plugin/matugen_colors.lua` | при следующем запуске |
| zathura | `~/.config/zathura/zathurarc` | при следующем запуске |
| spicetify | `~/.config/spicetify/Themes/text/color.ini` | `spicetify apply` |
| swaylock | `~/.config/swaylock/config` | при следующей блокировке |
| clipse | `~/.config/clipse/theme.json` | при следующем запуске |
| cal | `~/.local/bin/cal.sh` | при следующем запуске |
| greetd (`tuigreet`) | `/etc/tuigreet/config.toml` | статическая тема, только root |

Файлы matugen-шаблонов и готовые куски для установки — `contrib/theme/`.

## GTK3 и GTK4

matugen пишет `colors.css`, а GTK читает `gtk.css`. Без строки

```css
@import url("colors.css");
```

в `~/.config/gtk-3.0/gtk.css` и `~/.config/gtk-4.0/gtk.css` файл с палитрой
не подключается вовсе: приложения рисуются дефолтной темой Adwaita. Импорт
лежит в пользовательском `gtk.css`, поэтому matugen его не затирает.

Для GTK4 одного импорта мало. libadwaita 1.10 собирает цвета из переменных
`:root` (`--accent-bg-color`, `--window-bg-color`, …), а `@define-color`
из пользовательского CSS на акцент уже не влияет. Проверено на
`pavucontrol`: с прежним шаблоном, где были только `@define-color`, ползунки
оставались системно-синими `#3584e4`. Поэтому у GTK4 отдельный шаблон
`matugen-gtk4.css`: он задаёт и переменные, и `@define-color` — второе нужно
редким GTK4-приложениям без libadwaita.

## Qt

Ни `qt5ct`, ни `qt6ct` не нужны. В `qt6-base` и `qt5-base` уже есть плагин
`libqgtk3.so`; достаточно выставить в сессии

```kdl
environment {
    QT_QPA_PLATFORMTHEME "gtk3"
}
```

в `~/.config/niri/config.kdl`. Тогда Qt-приложение берёт палитру из GTK3
(`adw-gtk3-dark` + `colors.css`). Без переменной Qt6 рисует светлую
Fusion-тему: так выглядели `ark` и `qbittorrent` до правки.

Переменная действует на процессы, запущенные после перечитывания конфига
niri; уже открытые приложения нужно перезапустить. Стиль при этом
GTK-подобный, а не Breeze: это осознанный обмен, qt6ct не тащим.

## Светлая и тёмная схемы

Режим зафиксирован: `theme = "dark"` в `~/.config/matugen/config.toml`, а
`wall.sh` не передаёт `--mode`. Светлая схема технически получается
(`matugen --mode light`) и по контрасту проходит WCAG AA:

| Схема | text/base | subtext/base | primary/base |
| --- | --- | --- | --- |
| тёмная | 14.3 | 10.8 | 10.9 |
| светлая | 16.2 | 8.9 | 7.0 |

Но она меняет характер интерфейса: HUDbar — тёмное стекло, и светлая панель
поверх обоев читается иначе. Поэтому автоматический `--mode smart` не
включён; если понадобится, это одна строка в `wall.sh`, а не переделка
компонентов.

## Проверка 2026-10-28

- `pavucontrol` (GTK4): акцент был `#3584e4`, стал `#b8cacb` (`primary`).
- GTK3-проба (`Gtk.ProgressBar`): заливка `#b8cacb`, то есть `colors.css`
  подключён через импорт.
- `ark` (Qt6): до правки — светлое окно, после `QT_QPA_PLATFORMTHEME=gtk3`,
  унаследованной от niri, — тёмное, в цветах палитры.
- `file-roller` (GTK4): фон и текст из палитры, прозрачность `window-bg-color`
  сохранена.
- Контраст посчитан по `colors.css` и по временной светлой выгрузке matugen.

## Границы

- **Экран входа статичен.** `tuigreet` читает `/etc/tuigreet/config.toml`,
  файл принадлежит root. Динамическая тема потребовала бы писать в `/etc` из
  пользовательской сессии. Готовая тёмная тема лежит в
  `contrib/theme/tuigreet.toml`, ставится один раз:
  `sudo install -Dm644 tuigreet.toml /etc/tuigreet/config.toml`.
- **Уже запущенные приложения не перекрашиваются.** GTK и Qt читают тему при
  старте; HUDbar, niri и dunst перечитывают файлы сами.
- **`prefers-color-scheme` не переключается.** Тема всегда тёмная, поэтому
  `color-scheme` в gsettings остаётся `prefer-dark`.
- **Zen Browser** в этой работе не участвует: отложен в ROADMAP, D.
